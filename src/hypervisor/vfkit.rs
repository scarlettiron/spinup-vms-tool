//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Apple Virtualization on macOS, through vfkit (brew install vfkit).
//
//vfkit is one process per running VM, so there is no service holding VM
//definitions: each VM's settings live in ~/.vmseed/vms/<name>/vfkit.json, and
//`start` launches vfkit from them, detached, with a REST socket that `stop`
//and `status` talk to. Disks are sparse raw files in the same folder.
//Snapshots are APFS clones of those files (`cp -c`), taken while the VM is off.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{
    DiskInfo, Hypervisor, Snapshot, StopMode, Tagged, VmInfo, VmSpec, VmState, detach, mac_for,
    parse_tag, port_open, tag,
};
use crate::platform::HypervisorKind;
use crate::seed::disk::DiskFormat;

pub struct Vfkit;

const CONFIG_FILE: &str = "vfkit.json";

//What `start` needs to launch the VM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Machine {
    pub tag: String,
    pub cpus: u32,
    pub memory_mb: u64,
    pub mac: String,
    pub efi_vars: PathBuf,
    //(path, device id). The id appears in the guest as /dev/disk/by-id/virtio-<id>.
    pub disks: Vec<(PathBuf, String)>,
    pub iso: Option<PathBuf>,
    pub display: (u32, u32),
}

fn vm_dir(name: &str) -> Result<PathBuf> {
    Ok(crate::utilities::paths::user_path("vms")?.join(name))
}

fn load(name: &str) -> Result<Option<(PathBuf, Machine)>> {
    //The VM folder may be elsewhere (vm.path); the record knows where.
    let dir = match crate::vms::Record::load(name) {
        Ok(Some(record)) => record.vm_dir,
        _ => vm_dir(name)?,
    };
    let file = dir.join(CONFIG_FILE);
    if !file.is_file() {
        return Ok(None);
    }
    let machine: Machine = serde_json::from_str(&std::fs::read_to_string(&file)?)
        .with_context(|| format!("{} is damaged", file.display()))?;
    Ok(Some((dir, machine)))
}

fn save(dir: &Path, machine: &Machine) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join(CONFIG_FILE),
        serde_json::to_string_pretty(machine)?,
    )?;
    Ok(())
}

//The socket path. macOS limits these to about 104 bytes, so a long home folder
//falls back to a short path in /tmp.
fn socket_path(dir: &Path, name: &str) -> PathBuf {
    let preferred = dir.join("vfkit.sock");
    if preferred.as_os_str().len() < 100 {
        return preferred;
    }
    PathBuf::from(format!(
        "/tmp/{}-{}.sock",
        crate::brand::NAME,
        &mac_for(name).replace(':', "")[6..]
    ))
}

//The vfkit command line for a machine.
pub fn arguments(dir: &Path, name: &str, machine: &Machine, gui: bool) -> Vec<String> {
    let mut args = vec![
        "--cpus".to_string(),
        machine.cpus.to_string(),
        "--memory".to_string(),
        machine.memory_mb.to_string(),
        "--bootloader".to_string(),
        format!("efi,variable-store={},create", machine.efi_vars.display()),
    ];
    for (path, id) in &machine.disks {
        args.push("--device".to_string());
        args.push(format!("virtio-blk,path={},deviceId={id}", path.display()));
    }
    if let Some(iso) = &machine.iso {
        args.push("--device".to_string());
        args.push(format!("usb-mass-storage,path={},readonly", iso.display()));
    }
    args.extend([
        "--device".to_string(),
        format!("virtio-net,nat,mac={}", machine.mac),
        "--device".to_string(),
        "virtio-rng".to_string(),
        "--device".to_string(),
        format!(
            "virtio-serial,logFilePath={}",
            dir.join("console.log").display()
        ),
        "--restful-uri".to_string(),
        format!("unix://{}", socket_path(dir, name).display()),
    ]);
    if gui {
        args.extend([
            "--device".to_string(),
            format!(
                "virtio-gpu,width={},height={}",
                machine.display.0, machine.display.1
            ),
            "--device".to_string(),
            "virtio-input,keyboard".to_string(),
            "--device".to_string(),
            "virtio-input,pointing".to_string(),
            "--gui".to_string(),
        ]);
    }
    args
}

fn pid(dir: &Path) -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(dir.join("vfkit.pid"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let alive = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    alive.then_some(pid)
}

//One HTTP request over vfkit's unix socket. Returns the body.
#[cfg(unix)]
fn rest(socket: &Path, method: &str, body: Option<&str>) -> Result<String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("could not reach vfkit at {}", socket.display()))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let body = body.unwrap_or("");
    let request = format!(
        "{method} /vm/state HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    if !head.starts_with("HTTP/1.1 2") && !head.starts_with("HTTP/1.0 2") {
        bail!("vfkit answered: {}", head.lines().next().unwrap_or(""));
    }
    Ok(body.to_string())
}

#[cfg(not(unix))]
fn rest(_socket: &Path, _method: &str, _body: Option<&str>) -> Result<String> {
    bail!("vfkit only runs on macOS")
}

pub fn parse_rest_state(body: &str) -> VmState {
    #[derive(Deserialize)]
    struct State {
        state: String,
    }
    match serde_json::from_str::<State>(body).map(|s| s.state) {
        Ok(state) => match state.trim_start_matches("VirtualMachineState") {
            "Running" => VmState::Running,
            "Stopped" | "Error" => VmState::Off,
            "Paused" => VmState::Paused,
            other => VmState::Busy(other.to_string()),
        },
        Err(_) => VmState::Running,
    }
}

//The IP macOS's DHCP server gave a MAC address, from /var/db/dhcpd_leases.
pub fn lease_ip(leases: &str, mac: &str) -> Option<String> {
    //The leases file drops leading zeros: 52:54:0:a:b:c.
    let short: Vec<String> = mac
        .split(':')
        .map(|part| part.trim_start_matches('0').to_string())
        .map(|part| {
            if part.is_empty() {
                "0".to_string()
            } else {
                part
            }
        })
        .collect();
    let short = short.join(":");
    let mut ip: Option<String> = None;
    let mut found: Option<String> = None;
    for line in leases.lines() {
        let line = line.trim();
        if line == "{" {
            ip = None;
        } else if let Some(value) = line.strip_prefix("ip_address=") {
            ip = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("hw_address=") {
            let value = value.split_once(',').map(|(_, m)| m).unwrap_or(value);
            if value.eq_ignore_ascii_case(&short) || value.eq_ignore_ascii_case(mac) {
                found = ip.clone();
            }
        } else if line == "}" && found.is_some() {
            //The file lists the newest lease first.
            return found;
        }
    }
    found
}

fn clone_file(from: &Path, to: &Path) -> Result<()> {
    //cp -c makes an APFS clone: instant, and it takes no space until the copies
    //differ. Without APFS it falls back to a normal copy.
    let status = Command::new("cp").arg("-c").arg(from).arg(to).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => std::fs::copy(from, to)
            .map(|_| ())
            .with_context(|| format!("could not copy {}", from.display())),
    }
}

fn make_sparse(path: &Path, bytes: u64) -> Result<()> {
    let file = std::fs::File::create(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    file.set_len(bytes)?;
    Ok(())
}

impl Hypervisor for Vfkit {
    fn kind(&self) -> HypervisorKind {
        HypervisorKind::Vfkit
    }

    fn seed_format(&self) -> DiskFormat {
        DiskFormat::Raw
    }

    fn system_disk(&self) -> &'static str {
        "vda"
    }

    fn preflight(&self) -> Result<()> {
        if crate::utilities::paths::find_program("vfkit").is_none() {
            bail!("vfkit is not installed. Install it with:\n    brew install vfkit");
        }
        Ok(())
    }

    fn state(&self, name: &str) -> Result<VmState> {
        let Some((dir, _)) = load(name)? else {
            return Ok(VmState::Missing);
        };
        if pid(&dir).is_none() {
            return Ok(VmState::Off);
        }
        match rest(&socket_path(&dir, name), "GET", None) {
            Ok(body) => Ok(parse_rest_state(&body)),
            Err(_) => Ok(VmState::Busy("starting".to_string())),
        }
    }

    fn info(&self, name: &str) -> Result<VmInfo> {
        let Some((_, machine)) = load(name)? else {
            bail!("no VM named '{name}'");
        };
        let disks = machine
            .disks
            .iter()
            .map(|(path, _)| {
                let meta = std::fs::metadata(path).ok();
                DiskInfo {
                    path: path.display().to_string(),
                    size_bytes: meta.as_ref().map(|m| m.len()),
                    used_bytes: allocated(path),
                    location: None,
                }
            })
            .collect();
        Ok(VmInfo {
            cpus: Some(machine.cpus),
            memory_mb: Some(machine.memory_mb),
            ip: self.ip(name)?,
            disks,
            ..VmInfo::default()
        })
    }

    fn ip(&self, name: &str) -> Result<Option<String>> {
        let Some((_, machine)) = load(name)? else {
            return Ok(None);
        };
        let leases = std::fs::read_to_string("/var/db/dhcpd_leases").unwrap_or_default();
        Ok(lease_ip(&leases, &machine.mac))
    }

    fn create(&self, spec: &VmSpec) -> Result<()> {
        let dir = &spec.vm_dir;
        std::fs::create_dir_all(dir)?;
        let system = dir.join("disk.img");
        make_sparse(&system, spec.disk_gb << 30)?;
        let machine = Machine {
            tag: tag(&spec.project, spec.machine.as_deref()),
            cpus: spec.cpus,
            memory_mb: spec.memory_mb,
            mac: mac_for(&spec.name),
            efi_vars: dir.join("efi-vars.fd"),
            disks: vec![
                (system, "system".to_string()),
                (spec.seed.clone(), "seed".to_string()),
            ],
            iso: Some(spec.iso.clone()),
            display: spec.display,
        };
        save(dir, &machine)
    }

    fn finish_install(&self, spec: &VmSpec) -> Result<()> {
        let Some((dir, mut machine)) = load(&spec.name)? else {
            bail!("the VM '{}' has no {CONFIG_FILE}", spec.name);
        };
        machine.iso = None;
        machine.disks.retain(|(_, id)| id != "seed");
        for disk in &spec.data_disks {
            let path = dir.join(format!("data-{}.img", disk.label));
            if !path.exists() {
                make_sparse(&path, disk.size_gb << 30)?;
            }
            if !machine.disks.iter().any(|(_, id)| *id == disk.label) {
                machine.disks.push((path, disk.label.clone()));
            }
        }
        save(&dir, &machine)
    }

    fn system_disk_bytes(&self, name: &str) -> Option<u64> {
        let (dir, _) = load(name).ok()??;
        allocated(&dir.join("disk.img"))
    }

    fn start(&self, name: &str, with_console: bool) -> Result<()> {
        let Some((dir, machine)) = load(name)? else {
            bail!("no VM named '{name}'");
        };
        if pid(&dir).is_some() {
            return Ok(());
        }
        let program = crate::utilities::paths::find_program("vfkit")
            .context("vfkit is not installed (brew install vfkit)")?;
        let socket = socket_path(&dir, name);
        let _ = std::fs::remove_file(&socket);
        let log = std::fs::File::create(dir.join("vfkit.log"))?;
        let mut command = Command::new(program);
        command
            .args(arguments(&dir, name, &machine, with_console))
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        detach(&mut command);
        let child = command.spawn().context("could not start vfkit")?;
        std::fs::write(dir.join("vfkit.pid"), child.id().to_string())?;
        //vfkit exits at once on a bad option; say so instead of "starting".
        std::thread::sleep(std::time::Duration::from_millis(800));
        if pid(&dir).is_none() {
            let log = std::fs::read_to_string(dir.join("vfkit.log")).unwrap_or_default();
            bail!("vfkit stopped straight away:\n{}", log.trim());
        }
        Ok(())
    }

    fn stop(&self, name: &str, mode: StopMode) -> Result<()> {
        let Some((dir, _)) = load(name)? else {
            bail!("no VM named '{name}'");
        };
        let Some(pid) = pid(&dir) else {
            return Ok(());
        };
        let socket = socket_path(&dir, name);
        match mode {
            StopMode::Save => bail!(
                "Apple Virtualization (vfkit) can't save a VM's memory; use `stop` without --save"
            ),
            StopMode::Graceful => rest(&socket, "POST", Some(r#"{"state":"Stop"}"#)).map(|_| ()),
            StopMode::Force => rest(&socket, "POST", Some(r#"{"state":"HardStop"}"#))
                .map(|_| ())
                .or_else(|_| {
                    Command::new("kill")
                        .args(["-9", &pid.to_string()])
                        .status()?;
                    Ok(())
                }),
        }
    }

    fn remove(&self, name: &str, vm_dir: &Path, delete_disks: bool) -> Result<()> {
        if let Some((dir, _)) = load(name)? {
            if pid(&dir).is_some() {
                let _ = self.stop(name, StopMode::Force);
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            for file in [CONFIG_FILE, "vfkit.pid", "vfkit.sock", "desktop.rdp"] {
                let _ = std::fs::remove_file(dir.join(file));
            }
        }
        if delete_disks && vm_dir.is_dir() {
            std::fs::remove_dir_all(vm_dir)
                .with_context(|| format!("could not delete {}", vm_dir.display()))?;
        }
        Ok(())
    }

    fn open_console(&self, name: &str) -> Result<()> {
        match self.state(name)? {
            VmState::Off => self.start(name, true),
            VmState::Missing => bail!("no VM named '{name}'"),
            _ => bail!(
                "vfkit can only show a console window for a VM started with one. Stop it, then run \
                 `{} open --console {name}` to start it with a window.",
                crate::brand::NAME
            ),
        }
    }

    fn open_desktop(&self, name: &str, address: &str, user: &str) -> Result<()> {
        let dir = load(name)?.map(|(dir, _)| dir).unwrap_or(vm_dir(name)?);
        if !super::open_rdp(address, user, &dir)? {
            bail!(
                "could not open an RDP client. Install Microsoft's Windows App from the App Store, then connect to \
                 {address} as {user}."
            );
        }
        Ok(())
    }

    fn desktop_ready(&self, _name: &str, address: Option<&str>) -> bool {
        address.is_some_and(|a| port_open(a, 3389))
    }

    fn snapshot_create(&self, name: &str, label: &str) -> Result<()> {
        let Some((dir, machine)) = load(name)? else {
            bail!("no VM named '{name}'");
        };
        if pid(&dir).is_some() {
            bail!(
                "stop the VM before taking a snapshot (`{} stop {name}`)",
                crate::brand::NAME
            );
        }
        crate::utilities::names::check_snapshot_label(label)?;
        let target = dir.join("snapshots").join(label);
        if target.exists() {
            bail!("a snapshot called '{label}' already exists");
        }
        std::fs::create_dir_all(&target)?;
        for (path, _) in &machine.disks {
            clone_file(path, &target.join(path.file_name().unwrap_or_default()))?;
        }
        if machine.efi_vars.is_file() {
            clone_file(&machine.efi_vars, &target.join("efi-vars.fd"))?;
        }
        std::fs::write(
            target.join("created"),
            humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
        )?;
        Ok(())
    }

    fn snapshot_list(&self, name: &str) -> Result<Vec<Snapshot>> {
        let Some((dir, _)) = load(name)? else {
            bail!("no VM named '{name}'");
        };
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir.join("snapshots")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    list.push(Snapshot {
                        name: entry.file_name().to_string_lossy().to_string(),
                        created: std::fs::read_to_string(path.join("created"))
                            .ok()
                            .map(|s| s.trim().to_string()),
                    });
                }
            }
        }
        list.sort_by(|a, b| a.created.cmp(&b.created));
        Ok(list)
    }

    fn snapshot_restore(&self, name: &str, label: &str) -> Result<()> {
        let Some((dir, machine)) = load(name)? else {
            bail!("no VM named '{name}'");
        };
        let source = dir.join("snapshots").join(label);
        if !source.is_dir() {
            bail!("there is no snapshot called '{label}'");
        }
        if pid(&dir).is_some() {
            self.stop(name, StopMode::Force)?;
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        for (path, _) in &machine.disks {
            let saved = source.join(path.file_name().unwrap_or_default());
            if saved.is_file() {
                let _ = std::fs::remove_file(path);
                clone_file(&saved, path)?;
            }
        }
        let vars = source.join("efi-vars.fd");
        if vars.is_file() {
            let _ = std::fs::remove_file(&machine.efi_vars);
            clone_file(&vars, &machine.efi_vars)?;
        }
        Ok(())
    }

    fn snapshot_delete(&self, name: &str, label: &str) -> Result<()> {
        let Some((dir, _)) = load(name)? else {
            bail!("no VM named '{name}'");
        };
        let target = dir.join("snapshots").join(label);
        if !target.is_dir() {
            bail!("there is no snapshot called '{label}'");
        }
        std::fs::remove_dir_all(target)?;
        Ok(())
    }

    fn list_tagged(&self) -> Result<Vec<Tagged>> {
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(crate::utilities::paths::user_path("vms")?) else {
            return Ok(found);
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Ok(Some((_, machine))) = load(&name)
                && let Some(tagged) = parse_tag(&name, &machine.tag)
            {
                found.push(tagged);
            }
        }
        Ok(found)
    }
}

//Bytes a sparse file really uses on disk.
fn allocated(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(meta.blocks() * 512)
    }
    #[cfg(not(unix))]
    {
        Some(meta.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> Machine {
        Machine {
            tag: "vmseed::/Users/me/acme".to_string(),
            cpus: 4,
            memory_mb: 8192,
            mac: mac_for("acme"),
            efi_vars: PathBuf::from("/v/efi-vars.fd"),
            disks: vec![(PathBuf::from("/v/disk.img"), "system".to_string())],
            iso: Some(PathBuf::from("/i/u.iso")),
            display: (1920, 1080),
        }
    }

    #[test]
    fn command_lines() {
        let args = arguments(Path::new("/v"), "acme", &machine(), false).join(" ");
        assert!(args.contains("--bootloader efi,variable-store=/v/efi-vars.fd,create"));
        assert!(args.contains("virtio-blk,path=/v/disk.img,deviceId=system"));
        assert!(args.contains("usb-mass-storage,path=/i/u.iso,readonly"));
        assert!(args.contains(&format!("virtio-net,nat,mac={}", mac_for("acme"))));
        assert!(args.contains("--restful-uri unix:///v/vfkit.sock"));
        assert!(!args.contains("--gui"));
        assert!(
            arguments(Path::new("/v"), "acme", &machine(), true)
                .join(" ")
                .contains("--gui")
        );
    }

    #[test]
    fn macs_are_stable_and_local() {
        assert_eq!(mac_for("acme"), mac_for("acme"));
        assert_ne!(mac_for("acme"), mac_for("other"));
        assert!(mac_for("acme").starts_with("52:54:00:"));
    }

    #[test]
    fn leases_are_matched_without_leading_zeros() {
        let leases = "{\n\tname=old\n\tip_address=192.168.64.2\n\thw_address=1,52:54:0:a:b:c\n}\n{\n\tname=x\n\tip_address=192.168.64.9\n\thw_address=1,aa:bb:cc:dd:ee:ff\n}\n";
        assert_eq!(
            lease_ip(leases, "52:54:00:0a:0b:0c").as_deref(),
            Some("192.168.64.2")
        );
        assert_eq!(lease_ip(leases, "52:54:00:00:00:01"), None);
    }

    #[test]
    fn rest_states() {
        assert_eq!(
            parse_rest_state(r#"{"state":"VirtualMachineStateRunning"}"#),
            VmState::Running
        );
        assert_eq!(
            parse_rest_state(r#"{"state":"VirtualMachineStateStopped"}"#),
            VmState::Off
        );
    }

    #[test]
    fn long_folders_get_a_short_socket() {
        let long = PathBuf::from(format!("/Users/{}/.vmseed/vms/x", "a".repeat(100)));
        assert!(socket_path(&long, "x").starts_with("/tmp"));
    }
}
