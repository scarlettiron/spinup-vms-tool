//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//SSH keys on this computer.
//
//Vmseed keeps a key pair of its own in ~/.vmseed/ssh/ and puts the public half in
//every guest it builds, so `ssh`, `exec`, `logs` and `status` work without any
//setup. Its own known_hosts file sits beside it: a rebuilt VM gets a new host
//key under the same name, and keeping that out of ~/.ssh/known_hosts means a
//rebuild never trips ssh's "REMOTE HOST IDENTIFICATION HAS CHANGED" alarm for
//anything else.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::brand;
use crate::utilities::paths;

pub fn ssh_dir() -> Result<PathBuf> {
    paths::user_path("ssh")
}

pub fn private_key() -> Result<PathBuf> {
    Ok(ssh_dir()?.join("id_ed25519"))
}

pub fn known_hosts() -> Result<PathBuf> {
    Ok(ssh_dir()?.join("known_hosts"))
}

//Vmseed's public key, made on first use. None when ssh-keygen is not installed.
pub fn ensure_own_key() -> Result<Option<String>> {
    let private = private_key()?;
    let public = PathBuf::from(format!("{}.pub", private.display()));
    if !public.is_file() {
        let Some(keygen) = ssh_keygen() else {
            return Ok(None);
        };
        std::fs::create_dir_all(ssh_dir()?)?;
        let comment = format!("{}@{}", brand::NAME, local_hostname());
        let status = Command::new(keygen)
            .args(["-q", "-t", "ed25519", "-N", "", "-C", &comment, "-f"])
            .arg(&private)
            .stdin(Stdio::null())
            .status()
            .context("could not run ssh-keygen")?;
        if !status.success() {
            return Ok(None);
        }
        restrict_key_permissions(&private);
    }
    Ok(Some(std::fs::read_to_string(&public)?.trim().to_string()))
}

//The public half of a private key. Some(None) when the key has a passphrase;
//None when there is no ssh-keygen to ask.
pub fn public_from_private(path: &Path) -> Option<Option<String>> {
    let keygen = ssh_keygen()?;
    let output = Command::new(keygen)
        .args(["-y", "-P", "", "-f"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(if text.is_empty() { None } else { Some(text) })
    } else {
        Some(None)
    }
}

//Forgets a host in Vmseed's known_hosts, after a rebuild or a removal.
pub fn forget_host(names: &[&str]) -> Result<bool> {
    let path = known_hosts()?;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(false);
    };
    let mut changed = false;
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| {
            let hosts = line.split_whitespace().next().unwrap_or("");
            let matches = hosts.split(',').any(|host| {
                let host = host
                    .trim_start_matches('[')
                    .split(']')
                    .next()
                    .unwrap_or(host);
                names.iter().any(|name| {
                    host.eq_ignore_ascii_case(name)
                        || host.eq_ignore_ascii_case(&format!("{name}.local"))
                })
            });
            changed |= matches;
            !matches
        })
        .collect();
    if changed {
        let mut body = kept.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        std::fs::write(&path, body)?;
    }
    Ok(changed)
}

pub fn ssh_keygen() -> Option<PathBuf> {
    paths::find_program("ssh-keygen").or_else(|| {
        let candidates = [
            r"C:\Windows\System32\OpenSSH\ssh-keygen.exe",
            r"C:\Program Files\Git\usr\bin\ssh-keygen.exe",
        ];
        candidates
            .iter()
            .map(PathBuf::from)
            .find(|p| cfg!(windows) && p.is_file())
    })
}

pub fn ssh_program() -> Option<PathBuf> {
    paths::find_program("ssh").or_else(|| {
        let fallback = PathBuf::from(r"C:\Windows\System32\OpenSSH\ssh.exe");
        (cfg!(windows) && fallback.is_file()).then_some(fallback)
    })
}

fn local_hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "host".to_string())
}

//OpenSSH refuses a private key others can read.
fn restrict_key_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    {
        //ssh-keygen on Windows already writes an owner-only ACL.
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgetting_a_host_removes_only_its_lines() {
        let dir = std::env::temp_dir().join(format!("vmseed-kh-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ssh")).unwrap();
        // SAFETY: only this test in the binary sets VMSEED_HOME.
        unsafe { std::env::set_var("VMSEED_HOME", &dir) };
        std::fs::write(
            known_hosts().unwrap(),
            "acme.local ssh-ed25519 AAA\nother.local ssh-ed25519 BBB\n[acme.local]:22,10.0.0.5 ssh-rsa CCC\n",
        )
        .unwrap();
        assert!(forget_host(&["acme"]).unwrap());
        assert_eq!(
            std::fs::read_to_string(known_hosts().unwrap()).unwrap(),
            "other.local ssh-ed25519 BBB\n"
        );
        unsafe { std::env::remove_var("VMSEED_HOME") };
        let _ = std::fs::remove_dir_all(dir);
    }
}
