//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//A pretend hypervisor for tests. It keeps its VMs in a JSON file in the user
//folder, so the real program can be run command after command (create, then
//status, then stop) without a real VM. Only debug builds accept it: set
//VMSEED_HYPERVISOR=fake.
//
//An install "finishes" the first time anyone asks for the state after it
//starts, which is all `create`'s wait loop needs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{Hypervisor, Snapshot, StopMode, Tagged, VmInfo, VmSpec, VmState, parse_tag, tag};
use crate::platform::HypervisorKind;
use crate::seed::disk::DiskFormat;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FakeVm {
    state: String,
    installing: bool,
    installed: bool,
    snapshots: Vec<String>,
    tag: String,
    cpus: u32,
    memory_mb: u64,
    data_disks: Vec<String>,
}

pub struct Fake {
    vms: RefCell<BTreeMap<String, FakeVm>>,
}

fn file() -> PathBuf {
    crate::utilities::paths::user_path("fake-hypervisor.json")
        .unwrap_or_else(|_| PathBuf::from("fake-hypervisor.json"))
}

impl Fake {
    pub fn load() -> Fake {
        let vms = std::fs::read_to_string(file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Fake {
            vms: RefCell::new(vms),
        }
    }

    fn save(&self) -> Result<()> {
        let path = file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&*self.vms.borrow())?)?;
        Ok(())
    }

    fn with<T>(&self, name: &str, change: impl FnOnce(&mut FakeVm) -> Result<T>) -> Result<T> {
        let result = {
            let mut vms = self.vms.borrow_mut();
            let Some(vm) = vms.get_mut(name) else {
                bail!("no VM named '{name}'");
            };
            change(vm)?
        };
        self.save()?;
        Ok(result)
    }
}

impl Hypervisor for Fake {
    fn kind(&self) -> HypervisorKind {
        HypervisorKind::Fake
    }

    fn seed_format(&self) -> DiskFormat {
        DiskFormat::Raw
    }

    fn system_disk(&self) -> &'static str {
        "sda"
    }

    fn preflight(&self) -> Result<()> {
        Ok(())
    }

    fn state(&self, name: &str) -> Result<VmState> {
        if !self.vms.borrow().contains_key(name) {
            return Ok(VmState::Missing);
        }
        self.with(name, |vm| {
            let state = vm.state.clone();
            if vm.installing && state == "Running" {
                //The installer powers off when it is done.
                vm.installing = false;
                vm.installed = true;
                vm.state = "Off".to_string();
            }
            Ok(match state.as_str() {
                "Running" => VmState::Running,
                "Saved" => VmState::Saved,
                _ => VmState::Off,
            })
        })
    }

    fn info(&self, name: &str) -> Result<VmInfo> {
        let vms = self.vms.borrow();
        let Some(vm) = vms.get(name) else {
            bail!("no VM named '{name}'");
        };
        Ok(VmInfo {
            cpus: Some(vm.cpus),
            memory_mb: Some(vm.memory_mb),
            ip: (vm.state == "Running").then(|| "192.0.2.10".to_string()),
            ..VmInfo::default()
        })
    }

    fn ip(&self, name: &str) -> Result<Option<String>> {
        Ok(self.info(name)?.ip)
    }

    fn create(&self, spec: &VmSpec) -> Result<()> {
        if !spec.iso.is_file() || !spec.seed.is_file() {
            bail!("the fake hypervisor wants the ISO and the seed to exist");
        }
        self.vms.borrow_mut().insert(
            spec.name.clone(),
            FakeVm {
                state: "Off".to_string(),
                installing: true,
                tag: tag(&spec.project, spec.machine.as_deref()),
                cpus: spec.cpus,
                memory_mb: spec.memory_mb,
                ..FakeVm::default()
            },
        );
        self.save()
    }

    fn finish_install(&self, spec: &VmSpec) -> Result<()> {
        self.with(&spec.name, |vm| {
            if vm.state != "Off" {
                bail!("the VM must be off to finish the install");
            }
            vm.data_disks = spec.data_disks.iter().map(|d| d.label.clone()).collect();
            Ok(())
        })
    }

    fn system_disk_bytes(&self, _name: &str) -> Option<u64> {
        Some(0)
    }

    fn type_text(&self, _name: &str, _text: &str) -> Result<()> {
        Ok(())
    }

    fn start(&self, name: &str, _with_console: bool) -> Result<()> {
        self.with(name, |vm| {
            vm.state = "Running".to_string();
            Ok(())
        })
    }

    fn stop(&self, name: &str, mode: StopMode) -> Result<()> {
        self.with(name, |vm| {
            vm.state = if mode == StopMode::Save {
                "Saved"
            } else {
                "Off"
            }
            .to_string();
            Ok(())
        })
    }

    fn remove(&self, name: &str, vm_dir: &Path, delete_disks: bool) -> Result<()> {
        self.vms.borrow_mut().remove(name);
        self.save()?;
        if delete_disks && vm_dir.is_dir() {
            std::fs::remove_dir_all(vm_dir)?;
        }
        Ok(())
    }

    fn open_console(&self, _name: &str) -> Result<()> {
        Ok(())
    }

    fn open_desktop(&self, _name: &str, _address: &str, _user: &str) -> Result<()> {
        Ok(())
    }

    fn desktop_ready(&self, _name: &str, _address: Option<&str>) -> bool {
        true
    }

    fn snapshot_create(&self, name: &str, label: &str) -> Result<()> {
        self.with(name, |vm| {
            if vm.snapshots.iter().any(|s| s == label) {
                bail!("a snapshot called '{label}' already exists");
            }
            vm.snapshots.push(label.to_string());
            Ok(())
        })
    }

    fn snapshot_list(&self, name: &str) -> Result<Vec<Snapshot>> {
        let vms = self.vms.borrow();
        let Some(vm) = vms.get(name) else {
            bail!("no VM named '{name}'");
        };
        Ok(vm
            .snapshots
            .iter()
            .map(|s| Snapshot {
                name: s.clone(),
                created: None,
            })
            .collect())
    }

    fn snapshot_restore(&self, name: &str, label: &str) -> Result<()> {
        self.with(name, |vm| {
            if !vm.snapshots.iter().any(|s| s == label) {
                bail!("there is no snapshot called '{label}'");
            }
            vm.state = "Off".to_string();
            Ok(())
        })
    }

    fn snapshot_delete(&self, name: &str, label: &str) -> Result<()> {
        self.with(name, |vm| {
            let before = vm.snapshots.len();
            vm.snapshots.retain(|s| s != label);
            if vm.snapshots.len() == before {
                bail!("there is no snapshot called '{label}'");
            }
            Ok(())
        })
    }

    fn list_tagged(&self) -> Result<Vec<Tagged>> {
        Ok(self
            .vms
            .borrow()
            .iter()
            .filter_map(|(name, vm)| parse_tag(name, &vm.tag))
            .collect())
    }
}
