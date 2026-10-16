//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmseed list`: every VM on this computer, with its state, URLs and project,
//the way `ddev list` shows projects. Projects come from ~/.vmseed/projects.yaml
//(written by `init` and `create`) and VMs from their records; the state comes
//from the hypervisor. `--live` also asks each running guest about its database
//and bootstrap, in parallel. `--rescan` rebuilds the lists from the tags the
//hypervisor keeps on each VM, for when ~/.vmseed was lost.

use std::path::{Path, PathBuf};

use anstream::print;

use super::args::ListArgs;
use crate::brand;
use crate::config::ProjectConfig;
use crate::errors::AppResult;
use crate::guest;
use crate::hypervisor::{self, VmState};
use crate::registry::Registry;
use crate::utilities::{paths, text, ui::Ui};
use crate::vms::{Record, Target};

//What `list` shows for one VM.
struct Row {
    name: String,
    status: String,
    blueprint: String,
    url: Option<String>,
    adminer: Option<String>,
    project: PathBuf,
    project_name: String,
    machine: Option<String>,
    hostname: String,
    user: Option<String>,
    database: Option<String>,
    bootstrap: Option<String>,
}

pub fn run(args: ListArgs, ui: Ui) -> AppResult<()> {
    if args.rescan {
        rescan(ui)?;
    }
    ui.detail(&format!(
        "reading {}",
        paths::display_path(&paths::user_path(crate::constants::files::REGISTRY_FILE)?)
    ));
    let registry = Registry::load()?;
    let mut rows: Vec<Row> = Vec::new();
    for (project_name, entry) in &registry.projects {
        rows.extend(rows_for_project(
            project_name,
            &entry.path,
            &entry.blueprint,
        ));
    }
    //VMs whose project isn't registered (or was renamed) still show.
    for record in Record::all()? {
        if !rows.iter().any(|r| r.name == record.name) {
            rows.push(Row {
                name: record.name.clone(),
                status: String::new(),
                blueprint: record.blueprint.clone(),
                url: Some(format!("http://{}.local", record.hostname)),
                adminer: None,
                project_name: record
                    .project
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                project: record.project.clone(),
                machine: record.machine.clone(),
                hostname: record.hostname.clone(),
                user: record.user.clone(),
                database: None,
                bootstrap: None,
            });
        }
    }

    //Ask the hypervisor once per VM. A project without a VM yet says so.
    let host = hypervisor::host();
    for row in rows.iter_mut().filter(|r| r.status.is_empty()) {
        let hv = match Record::load(&row.name).ok().flatten() {
            Some(record) if record.hypervisor != host.kind() => {
                hypervisor::for_kind(record.hypervisor)
            }
            _ => hypervisor::host(),
        };
        row.status = match hv.state(&row.name) {
            Ok(state) => state.label(),
            Err(error) => {
                ui.detail(&format!("{}: {error:#}", row.name));
                "unknown".to_string()
            }
        };
    }

    if args.live {
        probe_all(&mut rows);
    }

    if ui.json {
        let items: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "name": row.name,
                    "project": row.project_name,
                    "machine": row.machine,
                    "status": row.status,
                    "blueprint": row.blueprint,
                    "url": row.url,
                    "adminer": row.adminer,
                    "path": row.project,
                    "database": row.database,
                    "bootstrap": row.bootstrap,
                })
            })
            .collect();
        ui.json(&serde_json::Value::Array(items));
        return Ok(());
    }

    if rows.is_empty() {
        ui.line(&format!(
            "No projects yet. Run `{} init` in a project folder to add one.",
            brand::NAME
        ));
        return Ok(());
    }

    let dash = || "—".to_string();
    let mut header = vec!["NAME", "STATUS", "BLUEPRINT", "URL", "ADMINER"];
    if args.live {
        header.extend(["DATABASE", "BOOTSTRAP"]);
    }
    header.push("PROJECT");
    let mut table = vec![header.into_iter().map(str::to_string).collect::<Vec<_>>()];
    for row in &rows {
        let mut line = vec![
            row.name.clone(),
            row.status.clone(),
            row.blueprint.clone(),
            row.url.clone().unwrap_or_else(dash),
            row.adminer.clone().unwrap_or_else(dash),
        ];
        if args.live {
            line.push(row.database.clone().unwrap_or_else(dash));
            line.push(row.bootstrap.clone().unwrap_or_else(dash));
        }
        let mut project = paths::display_path(&row.project);
        if row.status == "missing" {
            project.push_str(" (folder gone)");
        }
        line.push(project);
        table.push(line);
    }
    if !ui.quiet {
        print!("{}", text::table(&table));
    }
    Ok(())
}

//One row per VM the project defines: one, or one per Machines entry.
fn rows_for_project(project_name: &str, path: &Path, registered_blueprint: &str) -> Vec<Row> {
    let missing = |status: &str| Row {
        name: project_name.to_string(),
        status: status.to_string(),
        blueprint: registered_blueprint.to_string(),
        url: None,
        adminer: None,
        project: path.to_path_buf(),
        project_name: project_name.to_string(),
        machine: None,
        hostname: project_name.to_string(),
        user: None,
        database: None,
        bootstrap: None,
    };
    if !path.join(brand::DIR).is_dir() {
        return vec![missing("missing")];
    }
    let Ok(base) = ProjectConfig::load(path, None) else {
        return vec![missing("config error")];
    };
    let machines: Vec<Option<String>> = if base.machines.is_empty() {
        vec![None]
    } else {
        base.machines.iter().cloned().map(Some).collect()
    };
    machines
        .into_iter()
        .map(
            |machine| match ProjectConfig::load(path, machine.as_deref()) {
                Ok(config) => {
                    let names = config.names();
                    Row {
                        name: names.vm_name,
                        status: String::new(),
                        blueprint: config.blueprint(),
                        url: Some(config.url()),
                        adminer: config.adminer_url(),
                        project: path.to_path_buf(),
                        project_name: project_name.to_string(),
                        machine,
                        hostname: names.hostname,
                        user: crate::config::get_string(
                            &config.settings,
                            "Guest.PrimaryUser.Username",
                        ),
                        database: None,
                        bootstrap: None,
                    }
                }
                Err(_) => Row {
                    machine,
                    ..missing("config error")
                },
            },
        )
        .collect()
}

//Probes every running VM at once; a slow guest costs its own few seconds.
fn probe_all(rows: &mut [Row]) {
    std::thread::scope(|scope| {
        let handles: Vec<_> = rows
            .iter()
            .map(|row| {
                let running = row.status == "running";
                let user = row.user.clone();
                let hostname = row.hostname.clone();
                let name = row.name.clone();
                scope.spawn(move || {
                    if !running {
                        return None;
                    }
                    let hv = hypervisor::host();
                    let ip = hv.ip(&name).ok().flatten();
                    let address = guest::address(&hostname, ip.as_deref())?;
                    guest::probe(&guest::Target {
                        user: user?,
                        address,
                        alias: hostname.clone(),
                    })
                })
            })
            .collect();
        for (row, handle) in rows.iter_mut().zip(handles) {
            if let Ok(Some(facts)) = handle.join() {
                row.bootstrap = Some(facts.bootstrap().to_string());
                row.database = facts.0.get("database").map(|rows| {
                    let r = &rows[0];
                    format!(
                        "{} {} {}",
                        r[0],
                        r.get(3).cloned().unwrap_or_default(),
                        r.get(1).cloned().unwrap_or_default()
                    )
                });
            }
        }
    });
}

//Rebuilds the project list and VM records from the hypervisor's tags.
fn rescan(ui: Ui) -> AppResult<()> {
    let hv = hypervisor::host();
    let tagged = hv.list_tagged()?;
    let mut registry = Registry::load()?;
    let mut added = 0;
    for vm in &tagged {
        if !vm.project.join(brand::DIR).is_dir() {
            ui.warn(&format!(
                "{}: its project folder {} is gone",
                vm.name,
                paths::display_path(&vm.project)
            ));
            continue;
        }
        let config = ProjectConfig::load(&vm.project, vm.machine.as_deref()).ok();
        if !registry.projects.values().any(|p| p.path == vm.project) {
            let name = config
                .as_ref()
                .map(ProjectConfig::project_name)
                .unwrap_or_else(|| vm.name.clone());
            let blueprint = config
                .as_ref()
                .map(ProjectConfig::blueprint)
                .unwrap_or_default();
            registry.register(&name, &vm.project, &blueprint);
            added += 1;
        }
        if Record::load(&vm.name)?.is_none() {
            let target = Target {
                vm_name: vm.name.clone(),
                hostname: config
                    .as_ref()
                    .map(|c| c.names().hostname)
                    .unwrap_or(vm.name.clone()),
                user: config.as_ref().and_then(|c| {
                    crate::config::get_string(&c.settings, "Guest.PrimaryUser.Username")
                }),
                project: Some(vm.project.clone()),
                machine: vm.machine.clone(),
                record: None,
            };
            Record {
                name: vm.name.clone(),
                hostname: target.hostname.clone(),
                user: target.user.clone(),
                project: vm.project.clone(),
                machine: vm.machine.clone(),
                hypervisor: hv.kind(),
                blueprint: config
                    .as_ref()
                    .map(ProjectConfig::blueprint)
                    .unwrap_or_default(),
                vm_dir: paths::user_path("vms")?.join(&vm.name),
                created: String::new(),
                installed: hv
                    .state(&vm.name)
                    .map(|s| s != VmState::Missing)
                    .unwrap_or(false),
                spec: None,
            }
            .save()?;
        }
    }
    registry.save()?;
    ui.done(&format!(
        "found {} tagged VM(s); added {added} project(s)",
        tagged.len()
    ));
    Ok(())
}
