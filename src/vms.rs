//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The VMs this computer has built, and working out which one a command means.
//
//Every VM `create` builds gets a record, ~/.vmseed/vms/<name>/vm.json, naming
//its project, user and hypervisor. Commands then find a VM three ways
//(docs/vmseed-design.md section 12.2):
//
//  vmseed <cmd> <name>   a VM name or a Machines: key, from any folder
//  vmseed <cmd>          the project in this folder (all of its machines)
//  vmseed <cmd> --all    every VM on this computer

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::brand;
use crate::config::{ProjectConfig, get_string};
use crate::hypervisor::VmSpec;
use crate::platform::HypervisorKind;
use crate::registry::Registry;
use crate::utilities::paths;

const RECORD_FILE: &str = "vm.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub hostname: String,
    pub user: Option<String>,
    pub project: PathBuf,
    pub machine: Option<String>,
    pub hypervisor: HypervisorKind,
    pub blueprint: String,
    pub vm_dir: PathBuf,
    pub created: String,
    //False until the installer has finished and the VM booted from its disk.
    pub installed: bool,
    //What it was built with; absent for VMs found again by `list --rescan`.
    #[serde(default)]
    pub spec: Option<VmSpec>,
}

impl Record {
    fn path(name: &str) -> Result<PathBuf> {
        Ok(paths::user_path("vms")?.join(name).join(RECORD_FILE))
    }

    pub fn load(name: &str) -> Result<Option<Record>> {
        let path = Record::path(name)?;
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text)
            .map(Some)
            .with_context(|| format!("{} is damaged", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Record::path(&self.name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::utilities::files::write_atomic(&path, &serde_json::to_string_pretty(self)?)
    }

    pub fn delete(name: &str) -> Result<()> {
        let path = Record::path(name)?;
        if path.is_file() {
            std::fs::remove_file(&path)?;
        }
        //The folder too, when nothing else is in it.
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir(dir);
        }
        Ok(())
    }

    //Every record on this computer, sorted by name.
    pub fn all() -> Result<Vec<Record>> {
        let dir = paths::user_path("vms")?;
        let mut records = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(Some(record)) = Record::load(&name) {
                    records.push(record);
                }
            }
        }
        records.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(records)
    }
}

//One VM a command acts on, with what is known about it.
#[derive(Debug, Clone)]
pub struct Target {
    pub vm_name: String,
    pub hostname: String,
    pub user: Option<String>,
    pub project: Option<PathBuf>,
    pub machine: Option<String>,
    pub record: Option<Record>,
}

impl Target {
    fn from_record(record: Record) -> Target {
        Target {
            vm_name: record.name.clone(),
            hostname: record.hostname.clone(),
            user: record.user.clone(),
            project: Some(record.project.clone()),
            machine: record.machine.clone(),
            record: Some(record),
        }
    }

    fn from_config(config: &ProjectConfig) -> Target {
        let names = config.names();
        let record = Record::load(&names.vm_name).ok().flatten();
        Target {
            vm_name: names.vm_name,
            hostname: names.hostname,
            user: get_string(&config.settings, "Guest.PrimaryUser.Username"),
            project: Some(config.project_root.clone()),
            machine: config.machine.clone(),
            record,
        }
    }

    //The project and machine, for loading the full config again.
    pub fn project_and_machine(&self) -> Option<(PathBuf, Option<String>)> {
        self.project.clone().map(|p| (p, self.machine.clone()))
    }
}

//Every VM a project describes: one per machine, or its single VM.
pub fn project_targets(root: &Path) -> Result<Vec<Target>> {
    let base = ProjectConfig::load(root, None)?;
    if base.machines.is_empty() {
        return Ok(vec![Target::from_config(&base)]);
    }
    let mut targets = Vec::new();
    for machine in &base.machines {
        targets.push(Target::from_config(&ProjectConfig::load(
            root,
            Some(machine.as_str()),
        )?));
    }
    Ok(targets)
}

//Resolves a command's [name] and --all into VMs.
pub fn resolve(name: Option<&str>, all: bool) -> Result<Vec<Target>> {
    if all {
        let records = Record::all()?;
        return Ok(records.into_iter().map(Target::from_record).collect());
    }
    if let Some(name) = name {
        return Ok(vec![find(name)?]);
    }
    match paths::current_project_root()? {
        Some(root) => project_targets(&root),
        None => bail!(
            "this folder is not in a {app} project. Name a VM (`{app} list` shows them), use --all, \
             or run `{app} init` here.",
            app = brand::NAME
        ),
    }
}

//Resolves to exactly one VM, for commands that can only act on one.
pub fn resolve_one(name: Option<&str>) -> Result<Target> {
    let mut targets = resolve(name, false)?;
    if targets.len() > 1 {
        let names: Vec<String> = targets
            .iter()
            .map(|t| t.machine.clone().unwrap_or(t.vm_name.clone()))
            .collect();
        bail!(
            "this project has several machines ({}). Name the one you mean.",
            names.join(", ")
        );
    }
    Ok(targets.remove(0))
}

//A VM by its name or a Machines: key: the current project first, then every
//record, then every registered project.
fn find(name: &str) -> Result<Target> {
    if let Some(root) = paths::current_project_root()?
        && let Ok(targets) = project_targets(&root)
        && let Some(found) = targets.into_iter().find(|t| matches(t, name))
    {
        return Ok(found);
    }
    if let Some(record) = Record::load(name)? {
        return Ok(Target::from_record(record));
    }
    if let Some(record) = Record::all()?
        .into_iter()
        .find(|r| r.machine.as_deref() == Some(name))
    {
        return Ok(Target::from_record(record));
    }
    let registry = Registry::load()?;
    for entry in registry.projects.values() {
        if !entry.path.join(brand::DIR).is_dir() {
            continue;
        }
        if let Ok(targets) = project_targets(&entry.path)
            && let Some(found) = targets.into_iter().find(|t| matches(t, name))
        {
            return Ok(found);
        }
    }
    //Not one of ours, but the hypervisor may know it.
    Ok(Target {
        vm_name: name.to_string(),
        hostname: name.to_string(),
        user: None,
        project: None,
        machine: None,
        record: None,
    })
}

fn matches(target: &Target, name: &str) -> bool {
    target.vm_name.eq_ignore_ascii_case(name)
        || target
            .machine
            .as_deref()
            .is_some_and(|m| m.eq_ignore_ascii_case(name))
        || target.hostname.eq_ignore_ascii_case(name)
}
