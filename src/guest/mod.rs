//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Talking to a running guest over SSH: the status probe, `ssh`, `exec` and
//`logs`. Everything goes through the computer's own `ssh` program (OpenSSH
//ships with Windows 10+, macOS and every Linux), so there is no SSH library to
//keep secure.

pub mod keys;

use std::collections::BTreeMap;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::brand;

const PROBE: &str = include_str!("../../assets/guest/probe.sh");

//Who to log in as, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub user: String,
    pub address: String,
    //The guest's hostname. Host keys are filed under it, not the address, so
    //a reused DHCP address or a switch between name and IP never looks like a
    //changed key, and a rebuild only has to forget this one name.
    pub alias: String,
}

impl Target {
    pub fn display(&self) -> String {
        format!("{}@{}", self.user, self.address)
    }
}

//The address to use: <hostname>.local when this computer can resolve it,
//otherwise the address the hypervisor reported.
pub fn address(hostname: &str, ip: Option<&str>) -> Option<String> {
    let fqdn = format!("{hostname}.local");
    if resolves(&fqdn).is_some() {
        return Some(fqdn);
    }
    ip.map(str::to_string)
}

//What a name resolves to, IPv4 first. Never fails: not resolving is an answer.
pub fn resolves(name: &str) -> Option<String> {
    let addresses: Vec<_> = (name, 22).to_socket_addrs().ok()?.collect();
    addresses
        .iter()
        .find(|a| a.is_ipv4())
        .or(addresses.first())
        .map(|a| a.ip().to_string())
}

fn base_args(target: &Target, batch: bool) -> Result<Vec<String>> {
    let mut args = vec![
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        format!("UserKnownHostsFile={}", keys::known_hosts()?.display()),
        //Plain names in our own file (some systems hash them by default),
        //so `forget_host` can find them.
        "-o".to_string(),
        "HashKnownHosts=no".to_string(),
        "-o".to_string(),
        format!("HostKeyAlias={}", target.alias),
        "-o".to_string(),
        "LogLevel=ERROR".to_string(),
    ];
    let key = keys::private_key()?;
    if key.is_file() {
        args.push("-i".to_string());
        args.push(key.display().to_string());
    }
    if batch {
        for option in ["BatchMode=yes", "ConnectTimeout=4"] {
            args.push("-o".to_string());
            args.push(option.to_string());
        }
    }
    Ok(args)
}

fn ssh() -> Result<std::path::PathBuf> {
    keys::ssh_program().with_context(|| {
        "the `ssh` program is not installed. On Windows: Settings > System > Optional features > OpenSSH Client"
    })
}

//Runs a bash script in the guest and returns what it printed. None when the
//guest can't be reached, which callers treat as "unknown", never as an error.
pub fn run_script(target: &Target, script: &str) -> Option<String> {
    let program = ssh().ok()?;
    let mut child = Command::new(program)
        .args(base_args(target, true).ok()?)
        .arg(target.display())
        .arg("bash -s")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(script.as_bytes()).ok()?;
    let output = wait_with_timeout(child, Duration::from_secs(30))?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

fn wait_with_timeout(
    mut child: std::process::Child,
    limit: Duration,
) -> Option<std::process::Output> {
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100))
            }
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

//An interactive session, or one command with the terminal attached. Returns
//the remote command's exit code.
pub fn interactive(target: &Target, command: &[String]) -> Result<i32> {
    let mut process = Command::new(ssh()?);
    process.args(base_args(target, false)?);
    if command.is_empty() {
        process.arg("-t");
    }
    process.arg(target.display());
    if !command.is_empty() {
        process.arg("--").arg(shell_join(command));
    }
    let status = process.status().context("could not run ssh")?;
    Ok(status.code().unwrap_or(1))
}

//Joins arguments into one command line for the guest's shell, quoting where
//needed, so `exec -- ls "my folder"` arrives as two words.
pub fn shell_join(words: &[String]) -> String {
    if words.len() == 1 {
        //One argument is taken as a command line, as `ssh host 'a | b'` is.
        return words[0].clone();
    }
    words
        .iter()
        .map(|word| {
            if !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c))
            {
                word.clone()
            } else {
                format!("'{}'", word.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

//What the probe learned, by key. Each key may have several rows (users, disks).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts(pub BTreeMap<String, Vec<Vec<String>>>);

impl Facts {
    pub fn parse(text: &str) -> Facts {
        let mut facts: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
        for line in text.lines() {
            let mut parts = line.split('\t').map(str::to_string);
            let Some(key) = parts.next() else { continue };
            let fields: Vec<String> = parts.collect();
            if fields.is_empty() {
                continue;
            }
            facts.entry(key).or_default().push(fields);
        }
        Facts(facts)
    }

    //One field of the first row for `key`, if the guest said anything.
    pub fn get(&self, key: &str, field: usize) -> Option<&str> {
        self.0
            .get(key)?
            .first()?
            .get(field)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    pub fn rows(&self, key: &str) -> &[Vec<String>] {
        self.0.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    //"complete", "failed", "progress" or "unknown".
    pub fn bootstrap(&self) -> &str {
        self.get("bootstrap", 0).unwrap_or("unknown")
    }
}

//Asks a running guest everything the status page shows.
pub fn probe(target: &Target) -> Option<Facts> {
    run_script(target, PROBE).map(|text| Facts::parse(&text))
}

//The guest's bootstrap report, which lists every step with its result.
pub fn bootstrap_report(target: &Target) -> Option<String> {
    run_script(
        target,
        "cat /var/log/vm-bootstrap-report.txt 2>/dev/null || echo 'no report yet: bootstrap has not finished a run'\n",
    )
}

//The command `logs` runs: the bootstrap log, then the post-start hook log.
pub fn logs_command(follow: bool, lines: u32) -> String {
    let flags = if follow {
        format!("-n {lines} -F")
    } else {
        format!("-n {lines}")
    };
    //Only the logs that exist: the post-start log appears only with hooks.
    //sudo -n works when the user has passwordless sudo; plain tail otherwise
    //(the bootstrap log is readable by everyone).
    format!(
        "files=; for f in /var/log/vm-bootstrap.log /var/log/vm-post-start.log; do \
         [ -e \"$f\" ] && files=\"$files $f\"; done; \
         [ -n \"$files\" ] || {{ echo 'no logs yet: bootstrap has not started'; exit 0; }}; \
         sudo -n tail {flags} $files 2>/dev/null || tail {flags} $files"
    )
}

//Fails with a helpful message when there is no user to log in as.
pub fn require_user(user: Option<String>, vm: &str) -> Result<String> {
    match user {
        Some(user) => Ok(user),
        None => bail!(
            "no username is known for '{vm}'. Set `username:` in the project's .vmynt/vm.config.local.yaml \
             (see `{} config`).",
            brand::NAME
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_output_parses() {
        let facts = Facts::parse(
            "bootstrap\tprogress\t7\nos\tRocky Linux 9.6\nuser\tdev\tDeveloper\twheel\nuser\tci\t\t\ndf\t/\t100\t50\t50%\njunk\n",
        );
        assert_eq!(facts.bootstrap(), "progress");
        assert_eq!(facts.get("bootstrap", 1), Some("7"));
        assert_eq!(facts.get("os", 0), Some("Rocky Linux 9.6"));
        assert_eq!(facts.rows("user").len(), 2);
        assert_eq!(facts.get("missing", 0), None);
        assert_eq!(Facts::default().bootstrap(), "unknown");
    }

    #[test]
    fn words_are_quoted_for_the_guest_shell() {
        let words = |list: &[&str]| list.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert_eq!(
            shell_join(&words(&["ls", "-la", "my folder"])),
            "ls -la 'my folder'"
        );
        assert_eq!(shell_join(&words(&["echo", "it's"])), r"echo 'it'\''s'");
        assert_eq!(
            shell_join(&words(&["uname -a | tr a b"])),
            "uname -a | tr a b"
        );
    }

    #[test]
    fn the_probe_is_embedded() {
        assert!(PROBE.contains("bootstrap"));
    }
}
