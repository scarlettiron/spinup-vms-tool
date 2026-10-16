//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//One interface over the three hypervisors: Hyper-V on Windows, libvirt/KVM on
//Linux and Apple Virtualization (through vfkit) on macOS. Everything above this
//line (config, blueprints, the seed, the guest, the commands) is shared and
//never asks which one it is talking to. See docs/vmynt-design.md section 5.2.
//
//Every backend only runs other programs (powershell, virsh, vfkit), so all of
//them compile everywhere and their command building is tested on any computer;
//`host()` picks the one for this operating system.

pub mod fake;
pub mod hyperv;
pub mod libvirt;
pub mod vfkit;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::plans::DataDisk;
use crate::platform::{Arch, HypervisorKind};
use crate::seed::disk::DiskFormat;

//Everything a backend needs to define a VM. Built from the config by `create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSpec {
    pub name: String,
    pub hostname: String,
    pub arch: Arch,
    pub cpus: u32,
    pub memory_mb: u64,
    pub memory_min_mb: u64,
    pub memory_max_mb: u64,
    pub dynamic_memory: bool,
    pub disk_gb: u64,
    pub data_disks: Vec<DataDiskSpec>,
    pub secure_boot: bool,
    pub secure_boot_template: String,
    pub tpm: bool,
    pub nested: bool,
    pub enhanced_session: bool,
    pub auto_checkpoints: bool,
    //Hyper-V's virtual switch.
    pub switch_name: String,
    //libvirt's network and storage pool.
    pub network: String,
    pub storage_pool: String,
    //Where this VM's files live on this computer.
    pub vm_dir: PathBuf,
    pub iso: PathBuf,
    pub seed: PathBuf,
    //Written into the hypervisor's own notes, so `list --rescan` can find the
    //project again: "<project folder>" and the machine name, if any.
    pub project: PathBuf,
    pub machine: Option<String>,
    pub display: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataDiskSpec {
    pub label: String,
    pub size_gb: u64,
    pub location: u32,
}

impl From<&DataDisk> for DataDiskSpec {
    fn from(disk: &DataDisk) -> Self {
        DataDiskSpec {
            label: disk.label.clone(),
            size_gb: disk.size_gb,
            location: disk.location,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VmState {
    Running,
    Off,
    //Memory saved to disk; starts where it left off.
    Saved,
    Paused,
    //Starting, stopping, saving...
    Busy(String),
    //No such VM.
    Missing,
}

impl VmState {
    pub fn label(&self) -> String {
        match self {
            VmState::Running => "running".to_string(),
            VmState::Off => "stopped".to_string(),
            VmState::Saved => "saved".to_string(),
            VmState::Paused => "paused".to_string(),
            VmState::Busy(what) => what.to_lowercase(),
            VmState::Missing => "not created".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopMode {
    //Ask the guest to shut down.
    Graceful,
    //Pull the plug.
    Force,
    //Save memory to disk.
    Save,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DiskInfo {
    pub path: String,
    //The disk as the guest sees it, and what it takes on this computer.
    pub size_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    pub location: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct VmInfo {
    pub cpus: Option<u32>,
    pub memory_mb: Option<u64>,
    pub dynamic_memory: Option<(u64, u64)>,
    pub uptime_seconds: Option<u64>,
    pub ip: Option<String>,
    pub disks: Vec<DiskInfo>,
    //Hyper-V's heartbeat: the guest OS is properly up.
    pub heartbeat: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub name: String,
    pub created: Option<String>,
}

//A VM the hypervisor knows carries our tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tagged {
    pub name: String,
    pub project: PathBuf,
    pub machine: Option<String>,
}

pub trait Hypervisor {
    fn kind(&self) -> HypervisorKind;

    //How the seed disk must be written, and the installer's name for the
    //system disk ("sda" on SCSI, "vda" on virtio).
    fn seed_format(&self) -> DiskFormat;
    fn system_disk(&self) -> &'static str;
    //The seed disk's file name in the VM folder.
    fn seed_file_name(&self, name: &str) -> String {
        match self.seed_format() {
            DiskFormat::Vhd => format!("{name}-seed.vhd"),
            DiskFormat::Raw => format!("{name}-seed.img"),
        }
    }

    //Fails with instructions when this computer can't run VMs yet.
    fn preflight(&self) -> Result<()>;

    fn state(&self, name: &str) -> Result<VmState>;
    fn info(&self, name: &str) -> Result<VmInfo>;
    fn ip(&self, name: &str) -> Result<Option<String>>;

    //Defines the VM with the install media and seed attached, booting from the
    //ISO. Does not start it.
    fn create(&self, spec: &VmSpec) -> Result<()>;
    //After the installer powers off: eject the ISO, detach the seed, attach the
    //data disks (never during the install, so the installer can't pick or wipe
    //one), and boot from the system disk.
    fn finish_install(&self, spec: &VmSpec) -> Result<()>;
    //How much of the system disk has been written, for the install wait loop:
    //a disk that keeps growing is an install that is working.
    fn system_disk_bytes(&self, name: &str) -> Option<u64>;

    //Types text into the VM's keyboard, then Enter. Ubuntu's installer asks
    //"Continue with autoinstall?" and this is how it gets its answer.
    fn type_text(&self, name: &str, text: &str) -> Result<()> {
        let _ = (name, text);
        bail!("{} can't type into a VM", self.kind().display())
    }

    fn start(&self, name: &str, with_console: bool) -> Result<()>;
    //Sends the request; the caller polls `state` for the result.
    fn stop(&self, name: &str, mode: StopMode) -> Result<()>;
    fn remove(&self, name: &str, vm_dir: &Path, delete_disks: bool) -> Result<()>;

    fn open_console(&self, name: &str) -> Result<()>;
    //Opens the desktop. Hyper-V uses its own window (Enhanced Session); the
    //others use an RDP client pointed at the guest.
    fn open_desktop(&self, name: &str, address: &str, user: &str) -> Result<()>;
    //Whether the desktop is ready to open, from the host's point of view.
    fn desktop_ready(&self, name: &str, address: Option<&str>) -> bool;

    fn snapshot_create(&self, name: &str, label: &str) -> Result<()>;
    fn snapshot_list(&self, name: &str) -> Result<Vec<Snapshot>>;
    fn snapshot_restore(&self, name: &str, label: &str) -> Result<()>;
    fn snapshot_delete(&self, name: &str, label: &str) -> Result<()>;

    fn list_tagged(&self) -> Result<Vec<Tagged>>;
}

//The backend for this computer, or the one named by `kind`.
pub fn for_kind(kind: HypervisorKind) -> Box<dyn Hypervisor> {
    match kind {
        HypervisorKind::HyperV => Box::new(hyperv::HyperV),
        HypervisorKind::Libvirt => Box::new(libvirt::Libvirt::from_env()),
        HypervisorKind::Vfkit => Box::new(vfkit::Vfkit),
        HypervisorKind::Fake => Box::new(fake::Fake::load()),
    }
}

pub fn host() -> Box<dyn Hypervisor> {
    let kind = match std::env::var(format!("{}_HYPERVISOR", crate::brand::NAME.to_uppercase())) {
        Ok(value) => {
            HypervisorKind::from_setting(Some(&value)).unwrap_or_else(|_| HypervisorKind::host())
        }
        Err(_) => HypervisorKind::host(),
    };
    for_kind(kind)
}

//The text written into the hypervisor's notes for a VM, and read back by
//`list --rescan`.
pub fn tag(project: &Path, machine: Option<&str>) -> String {
    format!(
        "{}:{}:{}",
        crate::brand::NAME,
        machine.unwrap_or(""),
        project.display()
    )
}

pub fn parse_tag(name: &str, text: &str) -> Option<Tagged> {
    let rest = text
        .trim()
        .strip_prefix(&format!("{}:", crate::brand::NAME))?;
    let (machine, project) = rest.split_once(':')?;
    Some(Tagged {
        name: name.to_string(),
        project: PathBuf::from(project),
        machine: (!machine.is_empty()).then(|| machine.to_string()),
    })
}

//A MAC address that stays the same for a VM name, in the locally administered
//range, so the DHCP lease (and so the IP) is stable, and a guest network config
//that matches on it keeps working when the VM is redefined after the install.
pub fn mac_for(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(name.as_bytes());
    format!(
        "52:54:00:{:02x}:{:02x}:{:02x}",
        digest[0], digest[1], digest[2]
    )
}

//---------------------------------------------------------------- shared helpers

//Runs a program and returns its standard output. A failure carries the
//program's own error text.
pub fn run(program: &str, args: &[String]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("could not run {program}; is it installed?"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if stderr.is_empty() { stdout } else { stderr };
        bail!("{program} failed: {message}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

//Starts a program and leaves it running on its own (a viewer window).
pub fn spawn_detached(program: &Path, args: &[String]) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    command
        .spawn()
        .with_context(|| format!("could not start {}", program.display()))?;
    Ok(())
}

//Puts a child in its own process group, so Ctrl+C in this terminal does not
//reach it.
pub fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
}

//True when something answers on `address:port` within a couple of seconds.
pub fn port_open(address: &str, port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let Ok(addresses) = (address, port).to_socket_addrs() else {
        return false;
    };
    addresses
        .into_iter()
        .any(|a| TcpStream::connect_timeout(&a, Duration::from_secs(2)).is_ok())
}

//Opens an RDP client at the guest's xrdp (libvirt and vfkit guests listen on
//TCP 3389). Returns false when there is no client, so the caller can print the
//address instead.
pub fn open_rdp(address: &str, user: &str, vm_dir: &Path) -> Result<bool> {
    if cfg!(target_os = "macos") {
        //Microsoft's Windows App (formerly Remote Desktop) opens .rdp files.
        std::fs::create_dir_all(vm_dir)?;
        let file = vm_dir.join("desktop.rdp");
        std::fs::write(
            &file,
            format!(
                "full address:s:{address}\r\nusername:s:{user}\r\nprompt for credentials:i:1\r\n\
                 dynamic resolution:i:1\r\nredirectclipboard:i:1\r\nscreen mode id:i:1\r\n"
            ),
        )?;
        let opened = Command::new("open")
            .arg(&file)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        return Ok(opened);
    }
    if cfg!(windows) {
        let mstsc = PathBuf::from("mstsc.exe");
        spawn_detached(&mstsc, &[format!("/v:{address}")])?;
        return Ok(true);
    }
    for client in ["xfreerdp3", "xfreerdp", "wlfreerdp3", "wlfreerdp"] {
        if let Some(path) = crate::utilities::paths::find_program(client) {
            spawn_detached(
                &path,
                &[
                    format!("/v:{address}"),
                    format!("/u:{user}"),
                    "/dynamic-resolution".to_string(),
                    "+clipboard".to_string(),
                    "/cert:ignore".to_string(),
                ],
            )?;
            return Ok(true);
        }
    }
    if let Some(path) = crate::utilities::paths::find_program("remmina") {
        spawn_detached(
            &path,
            &["-c".to_string(), format!("rdp://{user}@{address}")],
        )?;
        return Ok(true);
    }
    Ok(false)
}

//"3.2 GB" from a byte count.
pub fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= (1u64 << 40) as f64 {
        format!("{:.1} TB", b / (1u64 << 40) as f64)
    } else if b >= (1u64 << 30) as f64 {
        format!("{:.1} GB", b / (1u64 << 30) as f64)
    } else if b >= (1u64 << 20) as f64 {
        format!("{:.0} MB", b / (1u64 << 20) as f64)
    } else {
        format!("{:.0} KB", b / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_round_trip() {
        let text = tag(Path::new("/home/me/acme"), Some("db"));
        let parsed = parse_tag("db", &text).unwrap();
        assert_eq!(parsed.project, PathBuf::from("/home/me/acme"));
        assert_eq!(parsed.machine.as_deref(), Some("db"));
        let plain = parse_tag("x", &tag(Path::new("C:\\work\\acme"), None)).unwrap();
        assert_eq!(plain.machine, None);
        assert_eq!(plain.project, PathBuf::from("C:\\work\\acme"));
        assert!(parse_tag("x", "something else").is_none());
    }

    #[test]
    fn sizes_read_like_people_write_them() {
        assert_eq!(human_size(3 << 30), "3.0 GB");
        assert_eq!(human_size(5 << 20), "5 MB");
    }
}
