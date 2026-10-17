//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Hyper-V, through PowerShell's Hyper-V cmdlets. Each operation is one
//PowerShell script that prints JSON, because starting powershell.exe costs
//about 300 ms.
//
//Never elevates itself. Hyper-V needs the user to be an administrator or in
//the "Hyper-V Administrators" group; the error points at `doctor --fix`
//(host_setup.rs), which asks before doing that.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde::Deserialize;

use super::{
    DiskInfo, Hypervisor, Snapshot, StopMode, Tagged, VmInfo, VmSpec, VmState, parse_tag,
    spawn_detached, tag,
};
use crate::plans::{LOCATION_DVD, LOCATION_OS, LOCATION_SEED};
use crate::platform::HypervisorKind;
use crate::seed::disk::DiskFormat;

pub struct HyperV;

//Quotes text for a PowerShell single-quoted string.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

//A script for powershell.exe -EncodedCommand (base64 of UTF-16LE), which needs
//no quoting however the script is written.
pub fn encode_command(script: &str) -> String {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

//Runs a script and returns what it printed. Errors come back as the cmdlet's
//own message.
pub fn powershell(script: &str) -> Result<String> {
    let full = format!(
        "$ErrorActionPreference = 'Stop'\n$ProgressPreference = 'SilentlyContinue'\n\
         try {{ Import-Module Hyper-V -ErrorAction Stop }} catch {{ [Console]::Error.WriteLine('HYPERV_MODULE_MISSING'); exit 3 }}\n\
         try {{\n{script}\n}} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}\n"
    );
    let encoded = encode_command(&full);
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .stdin(Stdio::null())
        .output()
        .context("could not run powershell.exe")?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        if stderr.contains("HYPERV_MODULE_MISSING") {
            bail!(
                "the Hyper-V PowerShell module is not installed, so Hyper-V is off. Run `{} doctor --fix` to turn it on.",
                crate::brand::NAME
            );
        }
        if stderr.contains("required permission")
            || stderr.contains("authorization policy")
            || stderr.contains("Access is denied")
        {
            bail!(
                "Hyper-V refused: you are not allowed to manage VMs. Run `{} doctor --fix` to add you to the \
                 Hyper-V Administrators group.\n({stderr})",
                crate::brand::NAME
            );
        }
        bail!(
            "{}",
            if stderr.is_empty() {
                "Hyper-V reported an error".to_string()
            } else {
                stderr
            }
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

//The script that defines a VM.
pub fn create_script(spec: &VmSpec) -> String {
    let name = quote(&spec.name);
    let dir = spec.vm_dir.display().to_string();
    let os_disk = quote(&format!("{dir}\\{}.vhdx", spec.name));
    let mut s = String::new();
    s.push_str(&format!(
        "if (-not (Get-VMSwitch -Name {switch} -ErrorAction SilentlyContinue)) {{\n\
         $have = (Get-VMSwitch | ForEach-Object {{ $_.Name }}) -join ', '\n\
         throw \"Virtual switch {switch_plain} does not exist. Available: $have. Set `switch:` in the config.\" }}\n",
        switch = quote(&spec.switch_name),
        switch_plain = spec.switch_name.replace('"', "")
    ));
    s.push_str(&format!(
        "New-Item -ItemType Directory -Force -Path {} | Out-Null\n",
        quote(&dir)
    ));
    s.push_str(&format!(
        "$vm = New-VM -Name {name} -Generation 2 -MemoryStartupBytes {mem}MB -NewVHDPath {os_disk} -NewVHDSizeBytes {disk}GB -SwitchName {switch} -Path {dir}\n",
        mem = spec.memory_mb,
        disk = spec.disk_gb,
        switch = quote(&spec.switch_name),
        dir = quote(&dir)
    ));
    s.push_str(&format!("Set-VMProcessor -VM $vm -Count {}\n", spec.cpus));
    let dynamic = spec.dynamic_memory && !spec.nested;
    if spec.nested {
        s.push_str("Set-VMProcessor -VM $vm -ExposeVirtualizationExtensions $true\n");
        s.push_str("Set-VMNetworkAdapter -VM $vm -MacAddressSpoofing On\n");
    }
    if dynamic {
        s.push_str(&format!(
            "Set-VMMemory -VM $vm -DynamicMemoryEnabled $true -MinimumBytes {}MB -StartupBytes {}MB -MaximumBytes {}MB\n",
            spec.memory_min_mb, spec.memory_mb, spec.memory_max_mb
        ));
    } else {
        s.push_str(&format!(
            "Set-VMMemory -VM $vm -DynamicMemoryEnabled $false -StartupBytes {}MB\n",
            spec.memory_mb
        ));
    }
    s.push_str(&format!(
        "Add-VMDvdDrive -VM $vm -ControllerNumber 0 -ControllerLocation {LOCATION_DVD} -Path {}\n",
        quote(&spec.iso.display().to_string())
    ));
    s.push_str(&format!(
        "Add-VMHardDiskDrive -VM $vm -Path {} -ControllerNumber 0 -ControllerLocation {LOCATION_SEED}\n",
        quote(&spec.seed.display().to_string())
    ));
    if spec.secure_boot {
        s.push_str(&format!(
            "Set-VMFirmware -VM $vm -EnableSecureBoot On -SecureBootTemplate {}\n",
            quote(&spec.secure_boot_template)
        ));
    } else {
        s.push_str("Set-VMFirmware -VM $vm -EnableSecureBoot Off\n");
    }
    if spec.tpm {
        s.push_str("Set-VMKeyProtector -VM $vm -NewLocalKeyProtector\nEnable-VMTPM -VM $vm\n");
    }
    s.push_str(&format!(
        "$dvd = Get-VMDvdDrive -VM $vm\n$hdd = Get-VMHardDiskDrive -VM $vm | Where-Object {{ $_.ControllerLocation -eq {LOCATION_OS} }}\n\
         Set-VMFirmware -VM $vm -BootOrder $dvd, $hdd\n"
    ));
    s.push_str(&format!(
        "Set-VM -VM $vm -AutomaticCheckpointsEnabled ${} -CheckpointType Standard\n",
        spec.auto_checkpoints
    ));
    s.push_str("Set-VM -VM $vm -AutomaticStartAction Nothing -AutomaticStopAction ShutDown\n");
    s.push_str(&format!(
        "Set-VM -VM $vm -Notes {}\n",
        quote(&tag(&spec.project, spec.machine.as_deref()))
    ));
    s.push_str(
        "foreach ($svc in @('Guest Service Interface', 'Heartbeat', 'Time Synchronization')) {\n\
         try { Enable-VMIntegrationService -VM $vm -Name $svc } catch { } }\n",
    );
    if spec.enhanced_session {
        s.push_str("try { Set-VM -VM $vm -EnhancedSessionTransportType HvSocket } catch { }\n");
    }
    s
}

//The script run after the installer powers off.
pub fn finish_script(spec: &VmSpec) -> String {
    let name = quote(&spec.name);
    let dir = spec.vm_dir.display().to_string();
    let mut s = format!(
        "$vm = Get-VM -Name {name}\n\
         if ($vm.State -ne 'Off') {{ throw \"Expected {plain} to be off before finishing the install; it is $($vm.State).\" }}\n\
         foreach ($d in @(Get-VMDvdDrive -VM $vm)) {{ Remove-VMDvdDrive -VMDvdDrive $d }}\n\
         Get-VMHardDiskDrive -VM $vm | Where-Object {{ $_.ControllerLocation -eq {LOCATION_SEED} }} | Remove-VMHardDiskDrive\n",
        plain = spec.name.replace('"', "")
    );
    for disk in &spec.data_disks {
        let path = quote(&format!("{dir}\\{}-{}.vhdx", spec.name, disk.label));
        s.push_str(&format!(
            "if (-not (Test-Path -LiteralPath {path})) {{ New-VHD -Path {path} -SizeBytes {size}GB -Dynamic | Out-Null }}\n\
             if (-not (Get-VMHardDiskDrive -VM $vm | Where-Object {{ $_.Path -eq {path} }})) {{\n\
             Add-VMHardDiskDrive -VM $vm -Path {path} -ControllerNumber 0 -ControllerLocation {location} }}\n",
            size = disk.size_gb,
            location = disk.location
        ));
    }
    s.push_str(&format!(
        "$hdd = Get-VMHardDiskDrive -VM $vm | Where-Object {{ $_.ControllerLocation -eq {LOCATION_OS} }}\n\
         Set-VMFirmware -VM $vm -FirstBootDevice $hdd\n"
    ));
    s
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InfoJson {
    state: String,
    processor_count: Option<u32>,
    memory_assigned: Option<u64>,
    memory_startup: Option<u64>,
    dynamic_memory_enabled: Option<bool>,
    memory_minimum: Option<u64>,
    memory_maximum: Option<u64>,
    uptime_seconds: Option<f64>,
    ip: Option<String>,
    heartbeat: Option<String>,
    #[serde(default)]
    disks: Vec<DiskJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DiskJson {
    path: String,
    controller_location: Option<u32>,
    size: Option<u64>,
    file_size: Option<u64>,
}

fn parse_state(text: &str) -> VmState {
    match text.trim() {
        "Running" => VmState::Running,
        "Off" => VmState::Off,
        "Saved" => VmState::Saved,
        "Paused" => VmState::Paused,
        "" | "Missing" => VmState::Missing,
        other => VmState::Busy(other.to_string()),
    }
}

const IP_SCRIPT: &str = "@(Get-VMNetworkAdapter -VM $vm | Select-Object -ExpandProperty IPAddresses | Where-Object { $_ -and $_ -notmatch ':' })[0]";

impl Hypervisor for HyperV {
    fn kind(&self) -> HypervisorKind {
        HypervisorKind::HyperV
    }

    fn seed_format(&self) -> DiskFormat {
        DiskFormat::Vhd
    }

    fn system_disk(&self) -> &'static str {
        "sda"
    }

    fn preflight(&self) -> Result<()> {
        powershell("Get-VMHost | Out-Null").map(|_| ())
    }

    fn state(&self, name: &str) -> Result<VmState> {
        let out = powershell(&format!(
            "$vm = Get-VM -Name {} -ErrorAction SilentlyContinue\nif ($vm) {{ [string]$vm.State }} else {{ 'Missing' }}",
            quote(name)
        ))?;
        Ok(parse_state(&out))
    }

    fn info(&self, name: &str) -> Result<VmInfo> {
        let out = powershell(&format!(
            "$vm = Get-VM -Name {name}\n\
             $hb = Get-VMIntegrationService -VM $vm -Name 'Heartbeat' -ErrorAction SilentlyContinue\n\
             $disks = @(Get-VMHardDiskDrive -VM $vm | ForEach-Object {{\n\
               $v = Get-VHD -Path $_.Path -ErrorAction SilentlyContinue\n\
               [pscustomobject]@{{ Path = $_.Path; ControllerLocation = $_.ControllerLocation; Size = $v.Size; FileSize = $v.FileSize }} }})\n\
             [pscustomobject]@{{ State = [string]$vm.State; ProcessorCount = $vm.ProcessorCount; MemoryAssigned = $vm.MemoryAssigned;\n\
               MemoryStartup = $vm.MemoryStartup; DynamicMemoryEnabled = $vm.DynamicMemoryEnabled;\n\
               MemoryMinimum = $vm.MemoryMinimum; MemoryMaximum = $vm.MemoryMaximum; UptimeSeconds = $vm.Uptime.TotalSeconds;\n\
               Ip = {IP_SCRIPT}; Heartbeat = [string]$hb.PrimaryStatusDescription; Disks = $disks }} | ConvertTo-Json -Depth 4 -Compress",
            name = quote(name)
        ))?;
        let json: InfoJson =
            serde_json::from_str(&out).context("could not read Hyper-V's answer")?;
        let running = json.state == "Running";
        let memory = if running && json.memory_assigned.unwrap_or(0) > 0 {
            json.memory_assigned
        } else {
            json.memory_startup
        };
        Ok(VmInfo {
            cpus: json.processor_count,
            memory_mb: memory.map(|b| b >> 20),
            dynamic_memory: json
                .dynamic_memory_enabled
                .filter(|on| *on)
                .and(json.memory_minimum.zip(json.memory_maximum))
                .map(|(min, max)| (min >> 20, max >> 20)),
            uptime_seconds: json.uptime_seconds.filter(|s| *s > 0.0).map(|s| s as u64),
            ip: json.ip.filter(|ip| !ip.is_empty()),
            heartbeat: json.heartbeat.filter(|h| !h.is_empty()).map(|h| h == "OK"),
            disks: json
                .disks
                .into_iter()
                .map(|d| DiskInfo {
                    path: d.path,
                    size_bytes: d.size,
                    used_bytes: d.file_size,
                    location: d.controller_location,
                })
                .collect(),
        })
    }

    fn ip(&self, name: &str) -> Result<Option<String>> {
        let out = powershell(&format!("$vm = Get-VM -Name {}\n{IP_SCRIPT}", quote(name)))?;
        Ok(Some(out).filter(|ip| !ip.is_empty()))
    }

    fn create(&self, spec: &VmSpec) -> Result<()> {
        powershell(&create_script(spec)).map(|_| ())
    }

    fn finish_install(&self, spec: &VmSpec) -> Result<()> {
        powershell(&finish_script(spec)).map(|_| ())
    }

    fn system_disk_bytes(&self, name: &str) -> Option<u64> {
        let out = powershell(&format!(
            "$vm = Get-VM -Name {}\n$d = Get-VMHardDiskDrive -VM $vm | Where-Object {{ $_.ControllerLocation -eq {LOCATION_OS} }} | Select-Object -First 1\n\
             (Get-Item -LiteralPath $d.Path).Length",
            quote(name)
        ))
        .ok()?;
        out.trim().parse().ok()
    }

    //Hyper-V's virtual keyboard, through its WMI class.
    fn type_text(&self, name: &str, text: &str) -> Result<()> {
        powershell(&format!(
            "$cs = Get-CimInstance -Namespace root\\virtualization\\v2 -ClassName Msvm_ComputerSystem -Filter \"ElementName='{}'\"\n\
             $kb = Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_Keyboard\n\
             Invoke-CimMethod -InputObject $kb -MethodName TypeText -Arguments @{{ asciiText = {} }} | Out-Null\n\
             Invoke-CimMethod -InputObject $kb -MethodName TypeKey -Arguments @{{ keyCode = [uint32]13 }} | Out-Null",
            name.replace('\'', ""),
            quote(text)
        ))
        .map(|_| ())
    }

    fn start(&self, name: &str, _with_console: bool) -> Result<()> {
        powershell(&format!("Start-VM -Name {} | Out-Null", quote(name))).map(|_| ())
    }

    fn stop(&self, name: &str, mode: StopMode) -> Result<()> {
        let n = quote(name);
        let script = match mode {
            //A saved or paused machine has no running OS to ask; resume it first.
            StopMode::Graceful => format!(
                "$vm = Get-VM -Name {n}\nif ($vm.State -in @('Saved', 'Paused')) {{ Start-VM -VM $vm | Out-Null; Start-Sleep -Seconds 5 }}\n\
                 Stop-VM -Name {n} -Force -AsJob | Out-Null"
            ),
            StopMode::Force => format!("Stop-VM -Name {n} -TurnOff -Force"),
            StopMode::Save => format!("Save-VM -Name {n}"),
        };
        powershell(&script).map(|_| ())
    }

    fn remove(&self, name: &str, vm_dir: &Path, delete_disks: bool) -> Result<()> {
        let n = quote(name);
        let delete = if delete_disks { "$true" } else { "$false" };
        powershell(&format!(
            "$vm = Get-VM -Name {n} -ErrorAction SilentlyContinue\n\
             if ($vm) {{\n\
               if ($vm.State -ne 'Off') {{ Stop-VM -VM $vm -TurnOff -Force -ErrorAction SilentlyContinue; Start-Sleep -Seconds 2 }}\n\
               $disks = @(Get-VMHardDiskDrive -VM $vm | Select-Object -ExpandProperty Path)\n\
               Get-VMSnapshot -VM $vm -ErrorAction SilentlyContinue | Remove-VMSnapshot -ErrorAction SilentlyContinue\n\
               Remove-VM -VM $vm -Force\n\
               if ({delete}) {{ foreach ($d in $disks) {{ if ($d -and (Test-Path -LiteralPath $d)) {{ Remove-Item -LiteralPath $d -Force -ErrorAction SilentlyContinue }} }} }}\n\
             }}\n\
             $dir = {dir}\n\
             if ({delete} -and (Test-Path -LiteralPath $dir)) {{\n\
               $left = @(Get-ChildItem -LiteralPath $dir -Recurse -File | Where-Object {{ $_.Extension -notin @('.vhdx', '.vhd', '.avhdx', '.vmcx', '.vmrs', '.vmgs', '.bin', '.vsv', '.json', '.cfg', '.rdp') -and $_.FullName -notmatch '\\\\build\\\\' }})\n\
               if ($left.Count -eq 0) {{ Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue }}\n\
             }}",
            dir = quote(&vm_dir.display().to_string())
        ))
        .map(|_| ())
    }

    fn open_console(&self, name: &str) -> Result<()> {
        let Some(vmconnect) = crate::utilities::paths::find_program("vmconnect") else {
            bail!(
                "vmconnect.exe is not installed. It comes with the Hyper-V management tools: run `{} doctor --fix` \
                 to turn them on.",
                crate::brand::NAME
            );
        };
        spawn_detached(&vmconnect, &["localhost".to_string(), name.to_string()])
    }

    //Hyper-V's own window gives an Enhanced Session (resizable, clipboard)
    //once the guest's xrdp is up, so the desktop is the console.
    fn open_desktop(&self, name: &str, _address: &str, _user: &str) -> Result<()> {
        self.open_console(name)
    }

    fn desktop_ready(&self, name: &str, _address: Option<&str>) -> bool {
        match self.info(name) {
            Ok(info) => info.ip.is_some() && info.heartbeat.unwrap_or(true),
            Err(_) => false,
        }
    }

    fn snapshot_create(&self, name: &str, label: &str) -> Result<()> {
        powershell(&format!(
            "Checkpoint-VM -Name {} -SnapshotName {}",
            quote(name),
            quote(label)
        ))
        .map(|_| ())
    }

    fn snapshot_list(&self, name: &str) -> Result<Vec<Snapshot>> {
        let out = powershell(&format!(
            "@(Get-VMSnapshot -VMName {} | ForEach-Object {{ [pscustomobject]@{{ Name = $_.Name; Created = $_.CreationTime.ToString('s') }} }}) | ConvertTo-Json -Compress",
            quote(name)
        ))?;
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Row {
            name: String,
            created: Option<String>,
        }
        if out.is_empty() {
            return Ok(Vec::new());
        }
        //ConvertTo-Json writes a lone object, not a list of one.
        let rows: Vec<Row> = serde_json::from_str(&out)
            .or_else(|_| serde_json::from_str::<Row>(&out).map(|r| vec![r]))
            .context("could not read Hyper-V's checkpoint list")?;
        Ok(rows
            .into_iter()
            .map(|r| Snapshot {
                name: r.name,
                created: r.created,
            })
            .collect())
    }

    fn snapshot_restore(&self, name: &str, label: &str) -> Result<()> {
        powershell(&format!(
            "Restore-VMSnapshot -VMName {} -Name {} -Confirm:$false",
            quote(name),
            quote(label)
        ))
        .map(|_| ())
    }

    fn snapshot_delete(&self, name: &str, label: &str) -> Result<()> {
        powershell(&format!(
            "Remove-VMSnapshot -VMName {} -Name {} -Confirm:$false",
            quote(name),
            quote(label)
        ))
        .map(|_| ())
    }

    fn list_tagged(&self) -> Result<Vec<Tagged>> {
        let out = powershell(&format!(
            "@(Get-VM | Where-Object {{ $_.Notes -like '{}:*' }} | ForEach-Object {{ [pscustomobject]@{{ Name = $_.Name; Notes = $_.Notes }} }}) | ConvertTo-Json -Compress",
            crate::brand::NAME
        ))?;
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Row {
            name: String,
            notes: String,
        }
        if out.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<Row> = serde_json::from_str(&out)
            .or_else(|_| serde_json::from_str::<Row>(&out).map(|r| vec![r]))
            .context("could not read Hyper-V's VM list")?;
        Ok(rows
            .into_iter()
            .filter_map(|r| parse_tag(&r.name, &r.notes))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hypervisor::DataDiskSpec;
    use crate::platform::Arch;
    use std::path::PathBuf;

    pub fn spec() -> VmSpec {
        VmSpec {
            name: "acme-rocky".to_string(),
            hostname: "acme-rocky".to_string(),
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
            secure_boot_template: "MicrosoftUEFICertificateAuthority".to_string(),
            tpm: false,
            nested: false,
            enhanced_session: true,
            auto_checkpoints: false,
            switch_name: "Default Switch".to_string(),
            network: "default".to_string(),
            storage_pool: "default".to_string(),
            vm_dir: PathBuf::from(r"C:\Users\me\.vmseed\vms\acme-rocky"),
            iso: PathBuf::from(r"C:\Users\me\.vmseed\isos\Rocky.iso"),
            seed: PathBuf::from(r"C:\Users\me\.vmseed\vms\acme-rocky\acme-rocky-seed.vhd"),
            project: PathBuf::from(r"C:\work\o'brien"),
            machine: None,
            display: (1920, 1080),
        }
    }

    #[test]
    fn create_script_defines_the_whole_vm() {
        let script = create_script(&spec());
        assert!(
            script.contains("New-VM -Name 'acme-rocky' -Generation 2 -MemoryStartupBytes 8192MB")
        );
        assert!(script.contains("-NewVHDSizeBytes 40GB -SwitchName 'Default Switch'"));
        assert!(
            script.contains("Add-VMDvdDrive -VM $vm -ControllerNumber 0 -ControllerLocation 1")
        );
        assert!(script.contains("-ControllerNumber 0 -ControllerLocation 2"));
        assert!(script.contains("-SecureBootTemplate 'MicrosoftUEFICertificateAuthority'"));
        assert!(
            script
                .contains("Set-VMMemory -VM $vm -DynamicMemoryEnabled $true -MinimumBytes 2048MB")
        );
        //Quotes in paths are doubled, not broken.
        assert!(script.contains("o''brien"));
        assert!(script.contains("EnhancedSessionTransportType HvSocket"));
    }

    #[test]
    fn nested_virtualization_forces_static_memory() {
        let mut s = spec();
        s.nested = true;
        let script = create_script(&s);
        assert!(script.contains("ExposeVirtualizationExtensions $true"));
        assert!(script.contains("DynamicMemoryEnabled $false"));
    }

    #[test]
    fn finishing_attaches_data_disks_and_ejects_media() {
        let script = finish_script(&spec());
        assert!(script.contains("Remove-VMDvdDrive"));
        assert!(script.contains("acme-rocky-drive-e.vhdx"));
        assert!(script.contains("-ControllerLocation 3"));
        assert!(script.contains("-FirstBootDevice $hdd"));
    }

    #[test]
    fn states_map() {
        assert_eq!(parse_state("Running"), VmState::Running);
        assert_eq!(parse_state("Missing"), VmState::Missing);
        assert_eq!(
            parse_state("Starting"),
            VmState::Busy("Starting".to_string())
        );
    }
}
