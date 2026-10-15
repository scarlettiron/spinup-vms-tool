//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt doctor`: checks this computer can run VMs, and says how to fix what it
//can't. Each check is one line. A failed required check makes the command exit
//with 1, so scripts and CI can use it.

use std::process::Command;

use anstream::println;

use crate::brand;
use crate::errors::{AppError, AppResult};
use crate::utilities::{
    paths,
    ui::{self, Ui},
};

//How one check turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Ok,
    //Works without it, but something is missing or worse.
    Warn,
    //Required, and missing.
    Fail,
}

#[derive(Debug, serde::Serialize)]
struct Check {
    name: String,
    outcome: Outcome,
    detail: String,
    //How to fix it, when it isn't Ok.
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
}

impl Check {
    fn ok(name: &str, detail: impl Into<String>) -> Self {
        Check {
            name: name.to_string(),
            outcome: Outcome::Ok,
            detail: detail.into(),
            fix: None,
        }
    }
    fn warn(name: &str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Check {
            name: name.to_string(),
            outcome: Outcome::Warn,
            detail: detail.into(),
            fix: Some(fix.into()),
        }
    }
    fn fail(name: &str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Check {
            name: name.to_string(),
            outcome: Outcome::Fail,
            detail: detail.into(),
            fix: Some(fix.into()),
        }
    }
}

//The real test: can the hypervisor be reached the way `create` will reach it?
fn check_hypervisor() -> Check {
    let hv = crate::hypervisor::host();
    let name = "hypervisor";
    match hv.preflight() {
        Ok(()) => Check::ok(name, format!("{} answers", hv.kind().display())),
        Err(error) => {
            let text = format!("{error:#}");
            let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
            Check::fail(name, first.to_string(), rest.trim().to_string())
        }
    }
}

pub fn run(ui: Ui) -> AppResult<()> {
    let mut checks = vec![check_ssh(), check_user_folder()];
    checks.extend(platform_checks());
    checks.push(check_hypervisor());

    if ui.json {
        let value = serde_json::to_value(&checks).map_err(anyhow::Error::from)?;
        ui.json(&value);
    } else if !ui.quiet {
        ui.heading(&format!(
            "{} doctor  ({} {})",
            brand::DISPLAY_NAME,
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        for check in &checks {
            let mark = match check.outcome {
                Outcome::Ok => ui::ok_mark(),
                Outcome::Warn => ui::warn_mark(),
                Outcome::Fail => ui::fail_mark(),
            };
            println!("{mark} {:<22} {}", check.name, check.detail);
            if let Some(fix) = &check.fix {
                ui.hint(&format!("    {fix}"));
            }
        }
    }

    let failed = checks
        .iter()
        .filter(|check| check.outcome == Outcome::Fail)
        .count();
    if failed == 0 {
        Ok(())
    } else {
        Err(AppError::Other(anyhow::anyhow!(
            "{failed} required check(s) failed"
        )))
    }
}

fn check_ssh() -> Check {
    match paths::find_program("ssh") {
        Some(path) => Check::ok("ssh", paths::display_path(&path)),
        None => Check::fail(
            "ssh",
            "not on the PATH",
            "Install OpenSSH. It is needed for status, ssh, exec and logs.",
        ),
    }
}

fn check_user_folder() -> Check {
    match paths::user_dir() {
        Ok(dir) => {
            let note = if dir.exists() {
                ""
            } else {
                " (created on first use)"
            };
            Check::ok(
                "data folder",
                format!("{}{note}", paths::display_path(&dir)),
            )
        }
        Err(error) => Check::fail(
            "data folder",
            error.to_string(),
            format!("Set {} to a folder you own.", brand::env_var("HOME")),
        ),
    }
}

//Runs a program and returns its standard output, or None if it could not run or
//failed.
fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(windows)]
fn platform_checks() -> Vec<Check> {
    let mut checks = Vec::new();

    //Get-VM only exists when the Hyper-V PowerShell module is installed, which
    //happens when Hyper-V is turned on.
    let hyperv = output_of(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[bool](Get-Command Get-VM -ErrorAction SilentlyContinue)",
        ],
    );
    checks.push(match hyperv.as_deref().map(str::trim) {
        Some("True") => Check::ok("Hyper-V", "enabled"),
        Some(_) => Check::fail(
            "Hyper-V",
            "not enabled",
            "Run as administrator: Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V -All, then restart. Windows Home has no Hyper-V.",
        ),
        None => Check::fail("Hyper-V", "could not run powershell.exe to check", "Make sure Windows PowerShell is installed."),
    });

    //S-1-5-32-578 is Hyper-V Administrators; S-1-16-12288 means this shell is
    //running elevated (as administrator).
    let groups = output_of("whoami.exe", &["/groups"]).unwrap_or_default();
    checks.push(if groups.contains("S-1-5-32-578") {
        Check::ok("permission", "member of Hyper-V Administrators")
    } else if groups.contains("S-1-16-12288") {
        Check::ok("permission", "running as administrator")
    } else {
        Check::fail(
            "permission",
            "not a Hyper-V Administrator",
            "Run as administrator once: Add-LocalGroupMember -SID S-1-5-32-578 -Member $env:USERNAME, then sign out and back in.",
        )
    });

    checks.push(match paths::find_program("vmconnect") {
        Some(_) => Check::ok("console", "vmconnect.exe"),
        None => Check::warn(
            "console",
            "vmconnect.exe not found",
            "It comes with the Hyper-V management tools.",
        ),
    });
    checks
}

#[cfg(target_os = "linux")]
fn platform_checks() -> Vec<Check> {
    use std::path::Path;

    let mut checks = Vec::new();

    let kvm = Path::new("/dev/kvm");
    checks.push(if !kvm.exists() {
        Check::fail("KVM", "/dev/kvm is missing", "Turn on virtualization (VT-x / AMD-V) in the firmware settings, and load the kvm module.")
    } else if std::fs::OpenOptions::new().read(true).write(true).open(kvm).is_err() {
        Check::fail("KVM", "/dev/kvm is not usable by you", "Add yourself to the kvm group: sudo usermod -aG kvm $USER, then log in again.")
    } else {
        Check::ok("KVM", "/dev/kvm")
    });

    checks.push(match paths::find_program("virsh") {
        Some(_) => Check::ok("libvirt", "virsh found"),
        None => Check::fail(
            "libvirt",
            "virsh not found",
            "Install libvirt and QEMU, e.g. sudo apt install libvirt-daemon-system qemu-system ovmf",
        ),
    });

    let groups = output_of("id", &["-nG"]).unwrap_or_default();
    checks.push(
        if groups.split_whitespace().any(|group| group == "libvirt") {
            Check::ok("permission", "member of the libvirt group")
        } else {
            Check::warn(
                "permission",
                "not in the libvirt group",
                "sudo usermod -aG libvirt $USER, then log in again.",
            )
        },
    );

    let firmware = [
        "/usr/share/OVMF",
        "/usr/share/edk2",
        "/usr/share/qemu/firmware",
        "/usr/share/edk2-ovmf",
    ];
    checks.push(match firmware.iter().find(|dir| Path::new(dir).exists()) {
        Some(dir) => Check::ok("UEFI firmware", *dir),
        None => Check::fail(
            "UEFI firmware",
            "OVMF not found",
            "Install OVMF, e.g. sudo apt install ovmf",
        ),
    });

    let rdp = ["xfreerdp3", "xfreerdp", "remmina"]
        .iter()
        .find_map(|name| paths::find_program(name));
    checks.push(match rdp {
        Some(path) => Check::ok("RDP client", paths::display_path(&path)),
        None => Check::warn(
            "RDP client",
            "none found",
            "Install FreeRDP or Remmina to open the desktop.",
        ),
    });
    checks
}

#[cfg(target_os = "macos")]
fn platform_checks() -> Vec<Check> {
    use std::path::Path;

    let mut checks = Vec::new();
    checks.push(match paths::find_program("vfkit") {
        Some(path) => Check::ok("vfkit", paths::display_path(&path)),
        None => Check::fail("vfkit", "not found", "brew install vfkit"),
    });
    let rdp = Path::new("/Applications/Windows App.app").exists()
        || Path::new("/Applications/Microsoft Remote Desktop.app").exists();
    checks.push(if rdp {
        Check::ok("RDP client", "Windows App")
    } else {
        Check::warn(
            "RDP client",
            "Windows App not found",
            "Install Microsoft's Windows App from the App Store to open the desktop.",
        )
    });
    if std::env::consts::ARCH == "aarch64" {
        checks.push(Check::ok("guests", "Apple Silicon runs arm64 guests"));
    }
    let _ = output_of;
    checks
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn platform_checks() -> Vec<Check> {
    let _ = output_of;
    vec![Check::fail(
        "platform",
        std::env::consts::OS,
        "Only Windows, Linux and macOS are supported.",
    )]
}
