//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//libvirt/KVM on Linux, through `virsh`.
//
//Uses the system connection (qemu:///system): only that one gets the `default`
//NAT network, which gives the guest an address this computer can reach and
//makes <hostname>.local work. Disks live in a libvirt storage pool (`default`,
//in /var/lib/libvirt/images) rather than in ~/.vmseed, because the system qemu
//runs as its own user and usually cannot read a home folder.
//
//Disks sit on a virtio-scsi controller at the same unit numbers Hyper-V uses,
//so the guest finds its data disks the same way on both (00-disks.sh). Each
//disk's serial is its label as well.

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::{
    DiskInfo, Hypervisor, Snapshot, StopMode, Tagged, VmInfo, VmSpec, VmState, mac_for, parse_tag,
    port_open, run, spawn_detached, tag,
};
use crate::plans::{LOCATION_DVD, LOCATION_OS, LOCATION_SEED};
use crate::platform::{Arch, HypervisorKind};
use crate::seed::disk::DiskFormat;

const NAMESPACE: &str = "https://scarlettiron.github.io/vmseed/ns/1";

pub struct Libvirt {
    uri: String,
}

impl Libvirt {
    pub fn from_env() -> Self {
        let key = format!("{}_LIBVIRT_URI", crate::brand::NAME.to_uppercase());
        Libvirt {
            uri: std::env::var(key).unwrap_or_else(|_| "qemu:///system".to_string()),
        }
    }

    fn virsh(&self, args: &[&str]) -> Result<String> {
        let mut all = vec!["-c".to_string(), self.uri.clone()];
        all.extend(args.iter().map(|a| a.to_string()));
        run("virsh", &all).map_err(|error| explain(error, &self.uri))
    }

    fn ensure_pool(&self, pool: &str) -> Result<()> {
        if self.virsh(&["pool-info", pool]).is_err() {
            self.virsh(&[
                "pool-define-as",
                pool,
                "dir",
                "--target",
                "/var/lib/libvirt/images",
            ])?;
            self.virsh(&["pool-build", pool])?;
        }
        let info = self.virsh(&["pool-info", pool])?;
        if !info
            .lines()
            .any(|l| l.starts_with("State:") && l.contains("running"))
        {
            self.virsh(&["pool-start", pool])?;
        }
        let _ = self.virsh(&["pool-autostart", pool]);
        Ok(())
    }

    fn ensure_network(&self, network: &str) -> Result<()> {
        let info = self.virsh(&["net-info", network]).with_context(|| {
            format!("libvirt has no '{network}' network. Install libvirt's default network (libvirt-daemon-config-network) or set vm.network")
        })?;
        if !info
            .lines()
            .any(|l| l.starts_with("Active:") && l.contains("yes"))
        {
            self.virsh(&["net-start", network])?;
        }
        let _ = self.virsh(&["net-autostart", network]);
        Ok(())
    }

    fn volume_path(&self, pool: &str, volume: &str) -> Option<String> {
        self.virsh(&["vol-path", "--pool", pool, volume])
            .ok()
            .map(|p| p.trim().to_string())
    }

    //Creates an empty volume and returns its path.
    fn create_volume(&self, pool: &str, volume: &str, bytes: u64, format: &str) -> Result<String> {
        if self.volume_path(pool, volume).is_some() {
            self.virsh(&["vol-delete", "--pool", pool, volume])?;
        }
        self.virsh(&[
            "vol-create-as",
            pool,
            volume,
            &format!("{bytes}b"),
            "--format",
            format,
        ])?;
        self.volume_path(pool, volume)
            .context("libvirt created a volume but can't say where it is")
    }

    //Copies a local file into the pool, so qemu can read it.
    fn upload(&self, pool: &str, volume: &str, file: &Path) -> Result<String> {
        let bytes = std::fs::metadata(file)?.len();
        let path = self.create_volume(pool, volume, bytes, "raw")?;
        self.virsh(&[
            "vol-upload",
            "--pool",
            pool,
            volume,
            &file.display().to_string(),
        ])?;
        Ok(path)
    }

    //The ISO, uploaded once and reused while its size matches.
    fn iso_volume(&self, pool: &str, iso: &Path) -> Result<String> {
        let name = iso
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let size = std::fs::metadata(iso)?.len();
        if let Some(path) = self.volume_path(pool, &name)
            && let Ok(info) = self.virsh(&["vol-info", "--bytes", "--pool", pool, &name])
            && info
                .lines()
                .any(|l| l.starts_with("Capacity:") && l.contains(&size.to_string()))
        {
            return Ok(path);
        }
        self.upload(pool, &name, iso)
    }

    //(target, file) for each disk (not CD-ROM) the domain has now.
    fn disk_sources(&self, name: &str) -> Vec<(String, String)> {
        let Ok(list) = self.virsh(&["domblklist", name, "--details"]) else {
            return Vec::new();
        };
        list.lines()
            .skip(2)
            .filter_map(|line| {
                let cols: Vec<&str> = line.split_whitespace().collect();
                (cols.len() >= 4 && cols[1] == "disk" && cols[3] != "-")
                    .then(|| (cols[2].to_string(), cols[3].to_string()))
            })
            .collect()
    }

    //The files a volume is layered on, from libvirt's description of it.
    fn backing_chain(&self, file: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut current = file.to_string();
        for _ in 0..32 {
            let Ok(xml) = self.virsh(&["vol-dumpxml", &current]) else {
                break;
            };
            let Some(next) = xml
                .split_once("<backingStore>")
                .and_then(|(_, rest)| rest.split_once("<path>"))
                .and_then(|(_, rest)| rest.split_once("</path>"))
                .map(|(path, _)| path.trim().to_string())
            else {
                break;
            };
            chain.push(next.clone());
            current = next;
        }
        chain
    }

    fn define(&self, xml: &str, vm_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(vm_dir)?;
        let file = vm_dir.join("domain.xml");
        std::fs::write(&file, xml)?;
        self.virsh(&["define", &file.display().to_string()])
            .map(|_| ())
    }
}

//Turns virsh's permission errors into the fix.
fn explain(error: anyhow::Error, uri: &str) -> anyhow::Error {
    let text = error.to_string();
    if text.contains("Permission denied")
        || text.contains("authentication")
        || text.contains("access denied")
    {
        anyhow::anyhow!(
            "libvirt refused the connection to {uri}. Add yourself to the libvirt group, then log out and back in:\n    \
             sudo usermod -aG libvirt $USER\n({text})"
        )
    } else if text.contains("could not run virsh") {
        anyhow::anyhow!(
            "virsh is not installed. Install libvirt and QEMU, for example:\n    \
             sudo apt install qemu-system libvirt-daemon-system ovmf      (Debian, Ubuntu)\n    \
             sudo dnf install @virtualization                            (Fedora, RHEL)"
        )
    } else {
        error
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

//What the domain is built from, beyond the spec: disk paths in the pool.
pub struct Disks {
    pub system: String,
    pub iso: Option<String>,
    pub seed: Option<String>,
    //(label, path, unit)
    pub data: Vec<(String, String, u32)>,
}

//The domain XML. `uuid` keeps the same identity when it is redefined after the
//install.
pub fn domain_xml(spec: &VmSpec, uuid: &str, disks: &Disks) -> String {
    let (arch, machine) = match spec.arch {
        Arch::X86_64 => ("x86_64", "q35"),
        Arch::Aarch64 => ("aarch64", "virt"),
    };
    let secure = spec.secure_boot && spec.arch == Arch::X86_64;
    let installing = disks.iso.is_some();
    let mut x = String::new();
    x.push_str(&format!(
        "<domain type='kvm'>\n  <name>{}</name>\n  <uuid>{uuid}</uuid>\n",
        escape(&spec.name)
    ));
    x.push_str(&format!(
        "  <metadata>\n    <vmseed:vm xmlns:vmseed='{NAMESPACE}'>{}</vmseed:vm>\n  </metadata>\n",
        escape(&tag(&spec.project, spec.machine.as_deref()))
    ));
    x.push_str(&format!(
        "  <memory unit='MiB'>{}</memory>\n",
        spec.memory_mb
    ));
    x.push_str(&format!("  <vcpu>{}</vcpu>\n", spec.cpus));
    x.push_str(&format!(
        "  <os firmware='efi'>\n    <type arch='{arch}' machine='{machine}'>hvm</type>\n"
    ));
    x.push_str(&format!(
        "    <firmware>\n      <feature enabled='{0}' name='secure-boot'/>\n      <feature enabled='{0}' name='enrolled-keys'/>\n    </firmware>\n",
        if secure { "yes" } else { "no" }
    ));
    x.push_str("  </os>\n  <features>\n    <acpi/>\n");
    if spec.arch == Arch::X86_64 {
        x.push_str("    <apic/>\n");
    }
    if secure {
        x.push_str("    <smm state='on'/>\n");
    }
    x.push_str("  </features>\n");
    x.push_str("  <cpu mode='host-passthrough'/>\n");
    x.push_str("  <clock offset='utc'/>\n  <on_poweroff>destroy</on_poweroff>\n  <on_reboot>restart</on_reboot>\n  <on_crash>destroy</on_crash>\n");
    x.push_str("  <devices>\n");
    x.push_str("    <controller type='scsi' index='0' model='virtio-scsi'/>\n");
    let disk = |x: &mut String,
                kind: &str,
                format: &str,
                path: &str,
                unit: u32,
                serial: &str,
                boot: Option<u32>,
                readonly: bool| {
        x.push_str(&format!("    <disk type='file' device='{kind}'>\n"));
        x.push_str(&format!(
            "      <driver name='qemu' type='{format}' discard='unmap'/>\n"
        ));
        x.push_str(&format!("      <source file='{}'/>\n", escape(path)));
        x.push_str(&format!(
            "      <target dev='sd{}' bus='scsi'/>\n",
            (b'a' + unit as u8) as char
        ));
        if !serial.is_empty() {
            x.push_str(&format!("      <serial>{}</serial>\n", escape(serial)));
        }
        if let Some(order) = boot {
            x.push_str(&format!("      <boot order='{order}'/>\n"));
        }
        if readonly {
            x.push_str("      <readonly/>\n");
        }
        x.push_str(&format!("      <address type='drive' controller='0' bus='0' target='0' unit='{unit}'/>\n    </disk>\n"));
    };
    disk(
        &mut x,
        "disk",
        "qcow2",
        &disks.system,
        LOCATION_OS,
        "system",
        Some(if installing { 2 } else { 1 }),
        false,
    );
    if let Some(iso) = &disks.iso {
        disk(&mut x, "cdrom", "raw", iso, LOCATION_DVD, "", Some(1), true);
    }
    if let Some(seed) = &disks.seed {
        disk(
            &mut x,
            "disk",
            "raw",
            seed,
            LOCATION_SEED,
            "seed",
            None,
            false,
        );
    }
    for (label, path, unit) in &disks.data {
        disk(&mut x, "disk", "qcow2", path, *unit, label, None, false);
    }
    x.push_str(&format!(
        "    <interface type='network'>\n      <mac address='{}'/>\n      <source network='{}'/>\n      <model type='virtio'/>\n    </interface>\n",
        mac_for(&spec.name),
        escape(&spec.network)
    ));
    x.push_str("    <channel type='unix'>\n      <target type='virtio' name='org.qemu.guest_agent.0'/>\n    </channel>\n");
    x.push_str("    <input type='tablet' bus='usb'/>\n    <input type='keyboard' bus='usb'/>\n");
    x.push_str("    <graphics type='vnc' autoport='yes' listen='127.0.0.1'/>\n");
    x.push_str("    <video>\n      <model type='virtio'/>\n    </video>\n");
    x.push_str("    <rng model='virtio'>\n      <backend model='random'>/dev/urandom</backend>\n    </rng>\n");
    if spec.tpm {
        x.push_str("    <tpm model='tpm-crb'>\n      <backend type='emulator' version='2.0'/>\n    </tpm>\n");
    }
    x.push_str("    <serial type='pty'/>\n    <console type='pty'/>\n");
    x.push_str("  </devices>\n</domain>\n");
    x
}

//Every `<source file='...'/>` in a domain or snapshot description.
pub fn source_files(xml: &str) -> Vec<String> {
    let mut found = Vec::new();
    for part in xml.split("<source file=").skip(1) {
        let mut chars = part.chars();
        let Some(quote) = chars.next() else { continue };
        if let Some(end) = chars.as_str().find(quote) {
            found.push(
                chars.as_str()[..end]
                    .replace("&amp;", "&")
                    .replace("&apos;", "'"),
            );
        }
    }
    found
}

//A UUID for a new domain, from the name and the time.
fn new_uuid(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    hasher.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            .to_le_bytes(),
    );
    let h = hasher.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&h[..16]);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

//The Linux key names `virsh send-key` takes for one character.
pub fn key_codes(c: char) -> Option<Vec<String>> {
    let key = |k: &str| format!("KEY_{k}");
    Some(match c {
        'a'..='z' | '0'..='9' => vec![key(&c.to_ascii_uppercase().to_string())],
        'A'..='Z' => vec![key("LEFTSHIFT"), key(&c.to_string())],
        ' ' => vec![key("SPACE")],
        '\n' => vec![key("ENTER")],
        '-' => vec![key("MINUS")],
        '.' => vec![key("DOT")],
        '/' => vec![key("SLASH")],
        _ => return None,
    })
}

fn parse_state(text: &str) -> VmState {
    match text.trim() {
        "running" => VmState::Running,
        "shut off" => VmState::Off,
        "paused" => VmState::Paused,
        "pmsuspended" => VmState::Saved,
        other => VmState::Busy(other.to_string()),
    }
}

//"Key:   value" lines from `virsh dominfo` and friends.
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .map(str::trim)
}

//The first IPv4 address in `virsh domifaddr` output.
pub fn parse_domifaddr(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 4 && cols[2] == "ipv4" {
            Some(cols[3].split('/').next()?.to_string())
        } else {
            None
        }
    })
}

impl Hypervisor for Libvirt {
    fn kind(&self) -> HypervisorKind {
        HypervisorKind::Libvirt
    }

    fn seed_format(&self) -> DiskFormat {
        DiskFormat::Raw
    }

    fn system_disk(&self) -> &'static str {
        "sda"
    }

    fn preflight(&self) -> Result<()> {
        if !Path::new("/dev/kvm").exists() {
            bail!(
                "/dev/kvm is missing, so this computer can't run fast VMs. Turn on virtualization (VT-x or AMD-V) in \
                 the firmware settings; inside a VM, turn on nested virtualization."
            );
        }
        self.virsh(&["version"]).map(|_| ())
    }

    fn state(&self, name: &str) -> Result<VmState> {
        match self.virsh(&["domstate", name]) {
            Ok(out) => {
                let state = parse_state(&out);
                if state == VmState::Off
                    && self
                        .virsh(&["dominfo", name])
                        .ok()
                        .and_then(|i| field(&i, "Managed save").map(|v| v == "yes"))
                        .unwrap_or(false)
                {
                    return Ok(VmState::Saved);
                }
                Ok(state)
            }
            Err(error)
                if error.to_string().contains("ailed to get domain")
                    || error.to_string().contains("not found") =>
            {
                Ok(VmState::Missing)
            }
            Err(error) => Err(error),
        }
    }

    fn info(&self, name: &str) -> Result<VmInfo> {
        let dominfo = self.virsh(&["dominfo", name])?;
        let cpus = field(&dominfo, "CPU(s)").and_then(|v| v.parse().ok());
        let memory_mb = field(&dominfo, "Used memory")
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map(|kib| kib / 1024);
        let mut disks = Vec::new();
        if let Ok(list) = self.virsh(&["domblklist", name, "--details"]) {
            for line in list.lines().skip(2) {
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() < 4 || cols[1] != "disk" {
                    continue;
                }
                let target = cols[2];
                let info = self
                    .virsh(&["domblkinfo", name, target])
                    .unwrap_or_default();
                let unit = target
                    .strip_prefix("sd")
                    .and_then(|l| l.bytes().next())
                    .map(|b| u32::from(b - b'a'));
                disks.push(DiskInfo {
                    path: cols[3].to_string(),
                    size_bytes: field(&info, "Capacity").and_then(|v| v.parse().ok()),
                    used_bytes: field(&info, "Physical").and_then(|v| v.parse().ok()),
                    location: unit,
                });
            }
        }
        Ok(VmInfo {
            cpus,
            memory_mb,
            dynamic_memory: None,
            uptime_seconds: None,
            ip: self.ip(name).ok().flatten(),
            disks,
            heartbeat: None,
        })
    }

    fn ip(&self, name: &str) -> Result<Option<String>> {
        for source in ["lease", "agent", "arp"] {
            if let Ok(out) = self.virsh(&["domifaddr", name, "--source", source])
                && let Some(ip) = parse_domifaddr(&out)
                && !ip.starts_with("127.")
            {
                return Ok(Some(ip));
            }
        }
        Ok(None)
    }

    fn create(&self, spec: &VmSpec) -> Result<()> {
        self.ensure_pool(&spec.storage_pool)?;
        self.ensure_network(&spec.network)?;
        let pool = &spec.storage_pool;
        let system = self.create_volume(
            pool,
            &format!("{}.qcow2", spec.name),
            spec.disk_gb << 30,
            "qcow2",
        )?;
        let seed = self.upload(pool, &format!("{}-seed.img", spec.name), &spec.seed)?;
        let iso = self.iso_volume(pool, &spec.iso)?;
        let uuid = new_uuid(&spec.name);
        std::fs::create_dir_all(&spec.vm_dir)?;
        std::fs::write(spec.vm_dir.join("uuid"), &uuid)?;
        std::fs::write(spec.vm_dir.join("pool"), pool)?;
        let xml = domain_xml(
            spec,
            &uuid,
            &Disks {
                system,
                iso: Some(iso),
                seed: Some(seed),
                data: Vec::new(),
            },
        );
        self.define(&xml, &spec.vm_dir)
    }

    fn finish_install(&self, spec: &VmSpec) -> Result<()> {
        let pool = &spec.storage_pool;
        let uuid = std::fs::read_to_string(spec.vm_dir.join("uuid"))
            .map(|u| u.trim().to_string())
            .or_else(|_| {
                self.virsh(&["domuuid", &spec.name])
                    .map(|u| u.trim().to_string())
            })?;
        //The disk the domain uses now, which is an overlay if a snapshot was
        //taken; writing to the base under it would break the snapshot.
        let system = self
            .disk_sources(&spec.name)
            .into_iter()
            .find(|(target, _)| target == "sda")
            .map(|(_, path)| path)
            .or_else(|| self.volume_path(pool, &format!("{}.qcow2", spec.name)))
            .context("the system disk is missing from the storage pool")?;
        let mut data = Vec::new();
        for disk in &spec.data_disks {
            let volume = format!("{}-{}.qcow2", spec.name, disk.label);
            let path = match self.volume_path(pool, &volume) {
                Some(path) => path,
                None => self.create_volume(pool, &volume, disk.size_gb << 30, "qcow2")?,
            };
            data.push((disk.label.clone(), path, disk.location));
        }
        let xml = domain_xml(
            spec,
            &uuid,
            &Disks {
                system,
                iso: None,
                seed: None,
                data,
            },
        );
        self.define(&xml, &spec.vm_dir)?;
        //The seed held the password hash and any secrets; it is not needed again.
        let _ = self.virsh(&[
            "vol-delete",
            "--pool",
            pool,
            &format!("{}-seed.img", spec.name),
        ]);
        Ok(())
    }

    fn system_disk_bytes(&self, name: &str) -> Option<u64> {
        let info = self.virsh(&["domblkinfo", name, "sda"]).ok()?;
        field(&info, "Allocation")?.parse().ok()
    }

    fn type_text(&self, name: &str, text: &str) -> Result<()> {
        //One send-key per character, since keys in one call are a chord.
        for c in text.chars().chain(std::iter::once('\n')) {
            let keys = key_codes(c).with_context(|| format!("can't type '{c}'"))?;
            let mut args = vec!["send-key", name];
            args.extend(keys.iter().map(String::as_str));
            self.virsh(&args)?;
        }
        Ok(())
    }

    fn start(&self, name: &str, _with_console: bool) -> Result<()> {
        self.virsh(&["start", name]).map(|_| ())
    }

    fn stop(&self, name: &str, mode: StopMode) -> Result<()> {
        match mode {
            StopMode::Graceful => self.virsh(&["shutdown", name]),
            StopMode::Force => self.virsh(&["destroy", name]),
            StopMode::Save => self.virsh(&["managedsave", name]),
        }
        .map(|_| ())
    }

    fn remove(&self, name: &str, vm_dir: &Path, delete_disks: bool) -> Result<()> {
        //Every file this VM uses: its disks now, what they are layered on, and
        //what each snapshot points at. Collected before the domain goes, and
        //deleted by exact path, so another VM's volumes (`acme-db` beside
        //`acme`) are never caught by a name pattern. ISOs are shared, and kept.
        let mut files: Vec<String> = Vec::new();
        if self.state(name)? != VmState::Missing {
            files.extend(self.disk_sources(name).into_iter().map(|(_, path)| path));
            if let Ok(snapshots) = self.virsh(&["snapshot-list", name, "--name"]) {
                for snapshot in snapshots.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    if let Ok(xml) = self.virsh(&["snapshot-dumpxml", name, snapshot]) {
                        files.extend(source_files(&xml));
                    }
                }
            }
            if self.state(name)? == VmState::Running {
                let _ = self.virsh(&["destroy", name]);
            }
        }
        //The last definition we wrote names the disks too, even after a failed build.
        if let Ok(xml) = std::fs::read_to_string(vm_dir.join("domain.xml")) {
            files.extend(source_files(&xml));
        }
        let mut chain = Vec::new();
        for file in &files {
            chain.extend(self.backing_chain(file));
        }
        files.extend(chain);
        files.retain(|f| !f.to_lowercase().ends_with(".iso"));
        files.sort();
        files.dedup();

        if self.state(name)? != VmState::Missing {
            //Snapshot metadata first; then the domain, its firmware variables
            //and any saved memory.
            if let Ok(snapshots) = self.virsh(&["snapshot-list", name, "--name"]) {
                for snapshot in snapshots.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    let _ = self.virsh(&["snapshot-delete", name, snapshot, "--metadata"]);
                }
            }
            self.virsh(&[
                "undefine",
                name,
                "--nvram",
                "--managed-save",
                "--snapshots-metadata",
            ])
            .or_else(|_| self.virsh(&["undefine", name, "--nvram"]))?;
        }
        if delete_disks {
            for file in files {
                let _ = self.virsh(&["vol-delete", &file]);
            }
            if vm_dir.is_dir() {
                std::fs::remove_dir_all(vm_dir).ok();
            }
        }
        Ok(())
    }

    fn open_console(&self, name: &str) -> Result<()> {
        let Some(viewer) = crate::utilities::paths::find_program("virt-viewer") else {
            bail!(
                "virt-viewer is not installed (sudo apt install virt-viewer, or sudo dnf install virt-viewer)"
            );
        };
        spawn_detached(
            &viewer,
            &[
                "--connect".to_string(),
                self.uri.clone(),
                "--attach".to_string(),
                name.to_string(),
            ],
        )
    }

    fn open_desktop(&self, _name: &str, address: &str, user: &str) -> Result<()> {
        let vm_dir = crate::utilities::paths::user_path("tmp")?;
        if !super::open_rdp(address, user, &vm_dir)? {
            bail!(
                "no RDP client found. Install FreeRDP (sudo apt install freerdp3-x11) or Remmina, or connect any RDP \
                 client to {address}:3389 as {user}."
            );
        }
        Ok(())
    }

    fn desktop_ready(&self, _name: &str, address: Option<&str>) -> bool {
        address.is_some_and(|a| port_open(a, 3389))
    }

    //External, disk-only snapshots, taken while the VM is off. Internal qcow2
    //snapshots can't be used: UEFI firmware variables live outside the disk.
    fn snapshot_create(&self, name: &str, label: &str) -> Result<()> {
        if self.state(name)? != VmState::Off {
            bail!(
                "stop the VM before taking a snapshot on libvirt (`{} stop`)",
                crate::brand::NAME
            );
        }
        self.virsh(&["snapshot-create-as", name, label, "--disk-only", "--atomic"])
            .map(|_| ())
    }

    fn snapshot_list(&self, name: &str) -> Result<Vec<Snapshot>> {
        let out = self.virsh(&["snapshot-list", name])?;
        Ok(out
            .lines()
            .skip(2)
            .filter_map(|line| {
                let cols: Vec<&str> = line.split_whitespace().collect();
                let name = cols.first()?.to_string();
                let created = (cols.len() >= 3).then(|| format!("{} {}", cols[1], cols[2]));
                Some(Snapshot { name, created })
            })
            .collect())
    }

    fn snapshot_restore(&self, name: &str, label: &str) -> Result<()> {
        if self.state(name)? == VmState::Running {
            self.virsh(&["destroy", name])?;
        }
        self.virsh(&["snapshot-revert", name, label])
            .map(|_| ())
            .map_err(|error| {
                if error.to_string().contains("external") {
                    anyhow::anyhow!(
                        "{error}\nReverting external snapshots needs libvirt 9.9 or newer."
                    )
                } else {
                    error
                }
            })
    }

    fn snapshot_delete(&self, name: &str, label: &str) -> Result<()> {
        self.virsh(&["snapshot-delete", name, label]).map(|_| ())
    }

    fn list_tagged(&self) -> Result<Vec<Tagged>> {
        let names = self.virsh(&["list", "--all", "--name"])?;
        let mut found = Vec::new();
        for name in names.lines().map(str::trim).filter(|n| !n.is_empty()) {
            if let Ok(text) = self.virsh(&["metadata", name, "--uri", NAMESPACE]) {
                let inner = text
                    .split_once('>')
                    .map(|(_, rest)| rest.rsplit_once('<').map(|(body, _)| body).unwrap_or(rest))
                    .unwrap_or("");
                let unescaped = inner
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&apos;", "'")
                    .replace("&quot;", "\"")
                    .replace("&amp;", "&");
                if let Some(tagged) = parse_tag(name, &unescaped) {
                    found.push(tagged);
                }
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hypervisor::DataDiskSpec;
    use std::path::PathBuf;

    fn spec() -> VmSpec {
        VmSpec {
            name: "acme-ubuntu".to_string(),
            hostname: "acme-ubuntu".to_string(),
            arch: Arch::X86_64,
            cpus: 4,
            memory_mb: 8192,
            memory_min_mb: 2048,
            memory_max_mb: 16384,
            dynamic_memory: true,
            disk_gb: 40,
            data_disks: vec![DataDiskSpec {
                label: "drive-e".to_string(),
                size_gb: 50,
                location: 3,
            }],
            secure_boot: true,
            secure_boot_template: String::new(),
            tpm: true,
            nested: false,
            enhanced_session: true,
            auto_checkpoints: false,
            switch_name: String::new(),
            network: "default".to_string(),
            storage_pool: "default".to_string(),
            vm_dir: PathBuf::from("/home/me/.vmseed/vms/acme-ubuntu"),
            iso: PathBuf::from("/home/me/.vmseed/isos/u.iso"),
            seed: PathBuf::from("/home/me/.vmseed/vms/acme-ubuntu/acme-ubuntu-seed.img"),
            project: PathBuf::from("/home/me/work/a&b"),
            machine: None,
            display: (1920, 1080),
        }
    }

    #[test]
    fn install_xml_boots_the_iso_with_the_seed_attached() {
        let xml = domain_xml(
            &spec(),
            "0000",
            &Disks {
                system: "/var/lib/libvirt/images/acme-ubuntu.qcow2".to_string(),
                iso: Some("/var/lib/libvirt/images/u.iso".to_string()),
                seed: Some("/var/lib/libvirt/images/acme-ubuntu-seed.img".to_string()),
                data: Vec::new(),
            },
        );
        assert!(xml.contains("<feature enabled='yes' name='secure-boot'/>"));
        assert!(xml.contains("<smm state='on'/>"));
        assert!(xml.contains("device='cdrom'"));
        assert!(xml.contains("unit='1'"));
        assert!(xml.contains("<serial>seed</serial>"));
        assert!(xml.contains("<boot order='1'/>\n      <readonly/>"));
        assert!(xml.contains("a&amp;b"), "metadata is escaped");
        assert!(xml.contains("tpm-crb"));
        assert!(
            xml.contains(&format!("<mac address='{}'/>", mac_for("acme-ubuntu"))),
            "the MAC survives a redefine"
        );
    }

    #[test]
    fn finished_xml_has_data_disks_at_their_luns() {
        let xml = domain_xml(
            &spec(),
            "0000",
            &Disks {
                system: "/p/acme-ubuntu.qcow2".to_string(),
                iso: None,
                seed: None,
                data: vec![(
                    "drive-e".to_string(),
                    "/p/acme-ubuntu-drive-e.qcow2".to_string(),
                    3,
                )],
            },
        );
        assert!(!xml.contains("cdrom"));
        assert!(xml.contains("<target dev='sdd' bus='scsi'/>\n      <serial>drive-e</serial>"));
        assert!(xml.contains("unit='3'"));
    }

    #[test]
    fn virsh_output_parses() {
        let out = " Name       MAC address          Protocol     Address\n-------------------------------------------------------\n vnet0      52:54:00:aa:bb:cc    ipv4         192.168.122.50/24\n";
        assert_eq!(parse_domifaddr(out).as_deref(), Some("192.168.122.50"));
        assert_eq!(
            field("CPU(s):         4\nState:  running\n", "CPU(s)"),
            Some("4")
        );
        assert_eq!(parse_state("shut off\n"), VmState::Off);
        let snapshot = "<disk name='sda' snapshot='external'>\n<source file='/p/a.clean-install'/>\n</disk><disk name='sdb'><source file=\"/p/u.iso\"/></disk>";
        assert_eq!(
            source_files(snapshot),
            vec!["/p/a.clean-install", "/p/u.iso"]
        );
        assert_eq!(key_codes('y').unwrap(), vec!["KEY_Y"]);
        assert_eq!(key_codes('Y').unwrap(), vec!["KEY_LEFTSHIFT", "KEY_Y"]);
        assert_eq!(key_codes('\n').unwrap(), vec!["KEY_ENTER"]);
        assert!(key_codes('$').is_none());
        let uuid = new_uuid("x");
        assert_eq!(uuid.len(), 36);
        assert_eq!(&uuid[14..15], "4");
    }
}
