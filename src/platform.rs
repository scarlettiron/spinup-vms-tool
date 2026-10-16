//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The two questions every build asks first: which CPU architecture is the guest,
//and which hypervisor runs it. Both come from config (`arch:` and
//`hypervisor:`), and both default to what this computer has.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    #[serde(rename = "x86_64")]
    X86_64,
    Aarch64,
}

impl Arch {
    //The architecture this program was built for, which is the computer's.
    pub fn host() -> Arch {
        if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else {
            Arch::X86_64
        }
    }

    //Reads `arch:` from config. Blank and "auto" mean the host's.
    pub fn from_setting(setting: Option<&str>) -> Result<Arch> {
        match setting.map(|s| s.trim().to_lowercase()).as_deref() {
            None | Some("") | Some("auto") => Ok(Arch::host()),
            Some("x86_64" | "x64" | "amd64") => Ok(Arch::X86_64),
            Some("aarch64" | "arm64") => Ok(Arch::Aarch64),
            Some(other) => bail!("arch '{other}' is not one of auto, x86_64 or arm64"),
        }
    }

    //The name the guest and the Linux world use.
    pub fn as_str(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HypervisorKind {
    #[serde(rename = "hyperv")]
    HyperV,
    Libvirt,
    Vfkit,
    //Only for tests: an in-memory hypervisor.
    Fake,
}

impl HypervisorKind {
    //The hypervisor this operating system uses.
    pub fn host() -> HypervisorKind {
        if cfg!(windows) {
            HypervisorKind::HyperV
        } else if cfg!(target_os = "macos") {
            HypervisorKind::Vfkit
        } else {
            HypervisorKind::Libvirt
        }
    }

    //Reads `hypervisor:` from config. Each operating system has one backend, so
    //anything but "auto" or that one is refused with the reason.
    pub fn from_setting(setting: Option<&str>) -> Result<HypervisorKind> {
        let host = HypervisorKind::host();
        let wanted = match setting.map(|s| s.trim().to_lowercase()).as_deref() {
            None | Some("") | Some("auto") => return Ok(host),
            Some("hyperv" | "hyper-v") => HypervisorKind::HyperV,
            Some("libvirt" | "kvm" | "qemu") => HypervisorKind::Libvirt,
            Some("vfkit" | "apple" | "virtualization") => HypervisorKind::Vfkit,
            Some("fake") if cfg!(debug_assertions) => HypervisorKind::Fake,
            Some(other) => {
                bail!("hypervisor '{other}' is not one of auto, hyperv, libvirt or vfkit")
            }
        };
        if wanted != host && wanted != HypervisorKind::Fake {
            bail!(
                "hypervisor '{}' does not run on this operating system; here it is '{}'. \
                 Remove `hypervisor:` from the config, or set it to auto.",
                wanted.as_str(),
                host.as_str()
            );
        }
        Ok(wanted)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            HypervisorKind::HyperV => "hyperv",
            HypervisorKind::Libvirt => "libvirt",
            HypervisorKind::Vfkit => "vfkit",
            HypervisorKind::Fake => "fake",
        }
    }

    //How people name it.
    pub fn display(self) -> &'static str {
        match self {
            HypervisorKind::HyperV => "Hyper-V",
            HypervisorKind::Libvirt => "libvirt/KVM",
            HypervisorKind::Vfkit => "Apple Virtualization (vfkit)",
            HypervisorKind::Fake => "fake",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arch_settings() {
        assert_eq!(Arch::from_setting(Some("arm64")).unwrap(), Arch::Aarch64);
        assert_eq!(Arch::from_setting(Some("AMD64")).unwrap(), Arch::X86_64);
        assert_eq!(Arch::from_setting(None).unwrap(), Arch::host());
        assert!(Arch::from_setting(Some("riscv")).is_err());
    }

    #[test]
    fn only_this_computers_hypervisor_is_accepted() {
        let host = HypervisorKind::host();
        assert_eq!(HypervisorKind::from_setting(Some("auto")).unwrap(), host);
        assert_eq!(
            HypervisorKind::from_setting(Some(host.as_str())).unwrap(),
            host
        );
        let other = if host == HypervisorKind::HyperV {
            "libvirt"
        } else {
            "hyperv"
        };
        assert!(HypervisorKind::from_setting(Some(other)).is_err());
    }
}
