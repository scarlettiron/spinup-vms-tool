//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt status`: what each VM is, where to reach it, and how bootstrap is
//going. This is Get-DevVMStatus.ps1. The hypervisor is the truth for hardware,
//config fills in what it can't know, and the guest (over ssh, when reachable)
//says what is really installed. Every fact is optional: an unreachable guest
//gives a partial report, never an error.

use std::time::Duration;

use anstream::{print, println};
use serde_json::json;

use super::args::StatusArgs;
use super::common::{again, hypervisor_for};
use crate::config::ProjectConfig;
use crate::errors::AppResult;
use crate::guest::{self, Facts};
use crate::hypervisor::{VmInfo, VmState, human_size};
use crate::utilities::ui::Ui;
use crate::vms::{self, Target};

const FOLLOW_EVERY: Duration = Duration::from_secs(10);

pub fn run(args: StatusArgs, ui: Ui) -> AppResult<()> {
    let targets = vms::resolve(args.target.name.as_deref(), args.target.all)?;
    loop {
        let reports: Vec<Report> = targets
            .iter()
            .map(|t| gather(t, !args.no_ssh, args.report))
            .collect();
        if ui.json {
            let values: Vec<serde_json::Value> = reports.iter().map(Report::json).collect();
            let value = if values.len() == 1 {
                values.into_iter().next().expect("one")
            } else {
                serde_json::Value::Array(values)
            };
            ui.json(&value);
        } else {
            if args.follow {
                print!("\x1b[2J\x1b[H");
            }
            for report in &reports {
                report.print(ui, args.report);
            }
        }
        let finished = reports
            .iter()
            .all(|r| r.state != VmState::Running || matches!(r.bootstrap(), "complete" | "failed"));
        if !args.follow || finished {
            break;
        }
        if !ui.json {
            ui.hint("  Ctrl+C to stop following.");
        }
        std::thread::sleep(FOLLOW_EVERY);
    }
    Ok(())
}

struct Report {
    target: Target,
    state: VmState,
    info: Option<VmInfo>,
    facts: Option<Facts>,
    resolved: Option<String>,
    config: Option<ProjectConfig>,
    ssh_tried: bool,
    report_text: Option<String>,
}

impl Report {
    fn bootstrap(&self) -> &str {
        self.facts
            .as_ref()
            .map(Facts::bootstrap)
            .unwrap_or("unknown")
    }

    fn json(&self) -> serde_json::Value {
        let facts = self.facts.as_ref();
        let get =
            |key: &str, field: usize| facts.and_then(|f| f.get(key, field)).map(str::to_string);
        json!({
            "name": self.target.vm_name,
            "hostname": self.target.hostname,
            "state": self.state.label(),
            "url": format!("http://{}.local", self.target.hostname),
            "resolves_to": self.resolved,
            "ip": self.info.as_ref().and_then(|i| i.ip.clone()),
            "user": self.target.user,
            "project": self.target.project,
            "machine": self.target.machine,
            "blueprint": self.target.record.as_ref().map(|r| r.blueprint.clone()),
            "cpus": self.info.as_ref().and_then(|i| i.cpus),
            "memory_mb": self.info.as_ref().and_then(|i| i.memory_mb),
            "uptime_seconds": self.info.as_ref().and_then(|i| i.uptime_seconds),
            "os": get("os", 0),
            "kernel": get("kernel", 0),
            "bootstrap": self.bootstrap(),
            "bootstrap_detail": get("bootstrap", 1),
            "database": facts.and_then(|f| f.0.get("database")).map(|rows| rows[0].clone()),
            "adminer": facts.and_then(|f| f.0.get("adminer")).map(|rows| rows[0].clone()),
            "github": facts.and_then(|f| f.0.get("github")).map(|rows| rows[0].clone()),
            "browser": get("browser", 0),
        })
    }

    fn print(&self, ui: Ui, with_report: bool) {
        let t = &self.target;
        println!();
        let uptime = self
            .info
            .as_ref()
            .and_then(|i| i.uptime_seconds)
            .map(|s| format!("  up {}h {:02}m", s / 3600, (s % 3600) / 60))
            .unwrap_or_default();
        ui.heading(&format!(
            "  {:<30} {}{uptime}",
            t.vm_name,
            self.state.label()
        ));
        let field = |label: &str, value: &str| ui.line(&format!("    {label:<11} {value}"));

        if self.state == VmState::Missing {
            if let Some(config) = &self.config {
                field("blueprint", &config.blueprint());
                field("build it", &again("create", t));
            }
            return;
        }

        let facts = self.facts.as_ref();
        let os = facts
            .and_then(|f| f.get("os", 0))
            .filter(|os| *os != "unknown")
            .map(str::to_string)
            .or_else(|| {
                self.config
                    .as_ref()
                    .map(|c| format!("{} (from config)", c.blueprint()))
            });
        if let Some(os) = os {
            field("os", &os);
        }
        if let Some(kernel) = facts.and_then(|f| f.get("kernel", 0)) {
            field("kernel", kernel);
        }
        if let Some(record) = &t.record {
            field("blueprint", &record.blueprint);
        }
        let ip = self.info.as_ref().and_then(|i| i.ip.clone());
        match (&self.resolved, &ip) {
            (Some(resolved), _) => field(
                "address",
                &format!("http://{}.local   (resolves to {resolved})", t.hostname),
            ),
            (None, Some(ip)) => field(
                "address",
                &format!(
                    "http://{ip}   ({}.local does not resolve from here yet)",
                    t.hostname
                ),
            ),
            (None, None) if self.state != VmState::Running => field(
                "address",
                &format!("http://{}.local   (once it is running)", t.hostname),
            ),
            (None, None) => field(
                "address",
                &format!(
                    "{}.local   (no address yet; mDNS may still be starting)",
                    t.hostname
                ),
            ),
        }
        if self.state == VmState::Running {
            field("ssh", &again("ssh", t));
        }

        let users = facts.map(|f| f.rows("user")).unwrap_or(&[]);
        if users.is_empty() {
            if let Some(user) = &t.user {
                field("user", &format!("{user}  (from config)"));
            }
        } else {
            for row in users {
                let real = row
                    .get(1)
                    .filter(|r| !r.is_empty())
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default();
                let groups = row
                    .get(2)
                    .filter(|g| !g.is_empty())
                    .map(|g| format!("   groups: {g}"))
                    .unwrap_or_default();
                field("user", &format!("{}{real}{groups}", row[0]));
            }
        }

        if let Some(info) = &self.info {
            let memory = info
                .memory_mb
                .map(|m| format!("{} RAM", human_size(m << 20)))
                .unwrap_or_default();
            let dynamic = info
                .dynamic_memory
                .map(|(min, max)| {
                    format!(
                        "   dynamic {} to {}",
                        human_size(min << 20),
                        human_size(max << 20)
                    )
                })
                .unwrap_or_default();
            field(
                "hardware",
                &format!("{} vCPU   {memory}{dynamic}", info.cpus.unwrap_or(0)),
            );
            if let Some(up) = facts.and_then(|f| f.get("guestuptime", 0)) {
                field("guest up", up);
            }
            let usage: Vec<&Vec<String>> = facts
                .map(|f| f.rows("df").iter().collect())
                .unwrap_or_default();
            for disk in &info.disks {
                let size = disk.size_bytes.map(human_size).unwrap_or("-".to_string());
                let used = disk.used_bytes.map(human_size).unwrap_or("-".to_string());
                let leaf = std::path::Path::new(&disk.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or(disk.path.clone());
                field(
                    "disk",
                    &format!("{leaf:<34} {size:>9} allocated, {used:>9} on this computer"),
                );
            }
            for row in usage {
                if row.len() >= 4 {
                    let total: u64 = row[1].parse().unwrap_or(0);
                    let used: u64 = row[2].parse().unwrap_or(0);
                    field(
                        "in use",
                        &format!(
                            "{:<12} {} of {} ({})",
                            row[0],
                            human_size(used),
                            human_size(total),
                            row[3]
                        ),
                    );
                }
            }
        }

        if let Some(row) = facts.and_then(|f| f.0.get("database")).map(|rows| &rows[0]) {
            let get = |i: usize| row.get(i).map(String::as_str).unwrap_or("");
            let reach = if get(5) == "*" {
                "reachable from other machines"
            } else {
                "localhost only"
            };
            let line = match get(1) {
                "running" => format!("{} {} running on port {}, {reach}", get(0), get(3), get(4)),
                "client-only" => format!("{} {} client tools only, no server", get(0), get(3)),
                "failed" => format!("{} {} FAILED - {}", get(0), get(3), get(2)),
                other => format!("{} {} {other} {}", get(0), get(3), get(2)),
            };
            field("database", &line);
        }
        if let Some(row) = facts.and_then(|f| f.0.get("adminer")).map(|rows| &rows[0]) {
            let get = |i: usize| row.get(i).map(String::as_str).unwrap_or("");
            match get(0) {
                "running" => field("adminer", &format!("{}   (adminer {})", get(1), get(2))),
                "failed" => field("adminer", &format!("FAILED - {}", get(1))),
                other => field("adminer", &format!("{other} {}", get(1))),
            }
        } else if let Some(url) = self.config.as_ref().and_then(ProjectConfig::adminer_url) {
            field("adminer", &format!("{url}  (from config)"));
        }
        if let Some(lines) = facts.and_then(|f| f.get("hooks", 0)) {
            let when = if facts.and_then(|f| f.get("hooks", 1)) == Some("enabled") {
                "runs on every boot"
            } else {
                "ran once"
            };
            field(
                "hooks",
                &format!("{when}, {lines} line(s) in /var/log/vm-post-start.log"),
            );
        }
        if let Some(status) = facts.and_then(|f| f.get("github", 0)) {
            let detail = facts.and_then(|f| f.get("github", 1)).unwrap_or("");
            let line = match status {
                "ok" => format!("authenticated as {detail}"),
                "mismatch" => format!("authenticated as {detail} - NOT the account you configured"),
                "denied" => format!("NOT authenticated - {detail}"),
                "skipped" => "no ssh key installed".to_string(),
                _ => format!("unverified - {detail}"),
            };
            field("github", &line);
        }
        if let Some(browser) = facts.and_then(|f| f.get("browser", 0)) {
            field("browser", browser);
        }

        if self.state != VmState::Running {
            return;
        }
        match (&self.facts, self.ssh_tried) {
            (None, false) => field("bootstrap", "(not checked: --no-ssh)"),
            (None, true) => field(
                "bootstrap",
                "(can't reach it over ssh yet; it may still be booting)",
            ),
            (Some(facts), _) => match facts.bootstrap() {
                "complete" => field("bootstrap", "complete"),
                "failed" => {
                    field(
                        "bootstrap",
                        &format!(
                            "finished with failed step(s): {}",
                            facts.get("bootstrap", 1).unwrap_or("see the report")
                        ),
                    );
                    field("", &format!("see which: {} --report", again("status", t)));
                }
                _ => field(
                    "bootstrap",
                    &format!(
                        "in progress ({} step(s) done)",
                        facts.get("bootstrap", 1).unwrap_or("0")
                    ),
                ),
            },
        }
        if with_report && let Some(text) = &self.report_text {
            println!();
            for line in text.lines() {
                ui.line(&format!("      {line}"));
            }
        }
    }
}

fn gather(target: &Target, use_ssh: bool, want_report: bool) -> Report {
    let hv = hypervisor_for(target);
    let state = hv.state(&target.vm_name).unwrap_or(VmState::Missing);
    let config = target
        .project
        .as_ref()
        .and_then(|root| ProjectConfig::load(root, target.machine.as_deref()).ok());
    let info = if state == VmState::Missing {
        None
    } else {
        hv.info(&target.vm_name).ok()
    };
    let resolved = guest::resolves(&format!("{}.local", target.hostname));
    let mut facts = None;
    let mut report_text = None;
    let ssh_tried = use_ssh && state == VmState::Running && target.user.is_some();
    if ssh_tried {
        let ip = info.as_ref().and_then(|i| i.ip.clone());
        if let (Some(user), Some(address)) = (
            target.user.clone(),
            guest::address(&target.hostname, ip.as_deref()),
        ) {
            let guest_target = guest::Target {
                user,
                address,
                alias: target.hostname.clone(),
            };
            facts = guest::probe(&guest_target);
            if facts.is_some() && want_report {
                report_text = guest::bootstrap_report(&guest_target);
            }
        }
    }
    Report {
        target: target.clone(),
        state,
        info,
        facts,
        resolved,
        config,
        ssh_tried,
        report_text,
    }
}
