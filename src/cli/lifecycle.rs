//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//start, stop, restart, poweroff and remove. None of them provisions anything:
//bootstrap runs once, on the first boot after `create`.

use std::time::Duration;

use super::args::{RemoveArgs, StopArgs, TargetArgs};
use super::common::{again, confirm, existing_state, hypervisor_for, minutes, wait_for};
use crate::brand;
use crate::errors::{AppError, AppResult};
use crate::guest::keys;
use crate::hypervisor::{Hypervisor, StopMode, VmState};
use crate::utilities::ui::Ui;
use crate::vms::{self, Record, Target};

const STOP_TIMEOUT: Duration = Duration::from_secs(120);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(180);

pub fn start(args: TargetArgs, ui: Ui) -> AppResult<()> {
    let targets = vms::resolve(args.name.as_deref(), args.all)?;
    let mut results = Vec::new();
    for target in &targets {
        results.push(start_one(target, ui, true)?);
    }
    if ui.json {
        ui.json(&serde_json::Value::Array(results));
    }
    Ok(())
}

//Starts one VM and waits for it to have an address.
pub fn start_one(target: &Target, ui: Ui, wait: bool) -> AppResult<serde_json::Value> {
    let hv = hypervisor_for(target);
    let state = existing_state(hv.as_ref(), target)?;
    let name = &target.vm_name;
    if state == VmState::Running {
        ui.done(&format!("{name} is already running"));
    } else {
        hv.start(name, false)?;
        let how = if state == VmState::Saved {
            "resumed from its saved state"
        } else {
            "started"
        };
        ui.done(&format!("{name} {how}"));
    }
    let mut ip = None;
    if wait && !ui.json {
        let start = std::time::Instant::now();
        while start.elapsed() < NETWORK_TIMEOUT {
            ip = hv.ip(name).ok().flatten();
            if ip.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_secs(3));
        }
        match &ip {
            Some(ip) => ui.info(&format!(
                "network up after {}, address {ip}",
                minutes(start.elapsed())
            )),
            None => ui.warn(&format!(
                "no address after {}s; it may still be booting. Check with `{}`.",
                NETWORK_TIMEOUT.as_secs(),
                again("status", target)
            )),
        }
    }
    if !ui.json {
        ui.line(&format!("  http://{}.local", target.hostname));
    }
    Ok(
        serde_json::json!({ "name": name, "state": "running", "ip": ip, "url": format!("http://{}.local", target.hostname) }),
    )
}

pub fn stop(args: StopArgs, ui: Ui) -> AppResult<()> {
    let mode = if args.save {
        StopMode::Save
    } else if args.force {
        StopMode::Force
    } else {
        StopMode::Graceful
    };
    let targets = vms::resolve(args.target.name.as_deref(), args.target.all)?;
    let mut failed = false;
    for target in &targets {
        if !stop_one(target, mode, ui)? {
            failed = true;
        }
    }
    if failed {
        return Err(AppError::Other(anyhow::anyhow!(
            "a VM did not shut down in time. Check its console, or force it with `{} stop --force`",
            brand::NAME
        )));
    }
    Ok(())
}

//Returns false when a graceful shutdown timed out.
pub fn stop_one(target: &Target, mode: StopMode, ui: Ui) -> AppResult<bool> {
    let hv = hypervisor_for(target);
    let name = &target.vm_name;
    let state = existing_state(hv.as_ref(), target)?;
    if state == VmState::Off {
        ui.done(&format!("{name} is already stopped"));
        return Ok(true);
    }
    if state == VmState::Saved && mode == StopMode::Save {
        ui.done(&format!("{name} is already saved"));
        return Ok(true);
    }
    match mode {
        StopMode::Save => ui.info("saving its memory to disk, so it resumes where it left off"),
        StopMode::Force => ui.warn(&format!("turning {name} off without a clean shutdown")),
        StopMode::Graceful => ui.info("asking the guest to shut down"),
    }
    hv.stop(name, mode)?;
    let want = if mode == StopMode::Save {
        VmState::Saved
    } else {
        VmState::Off
    };
    let done = wait_for(
        hv.as_ref(),
        name,
        STOP_TIMEOUT,
        Duration::from_secs(15),
        ui,
        |state| *state == want,
        |elapsed| format!("still shutting down, {} elapsed", minutes(elapsed)),
    )?;
    if done {
        ui.done(&format!(
            "{name} {}",
            if mode == StopMode::Save {
                "saved"
            } else {
                "stopped"
            }
        ));
    } else {
        ui.warn(&format!(
            "{name} is still not off after {}s. The guest is ignoring the request; check `{}`, or force it with `{} --force`.",
            STOP_TIMEOUT.as_secs(),
            again("open --console", target),
            again("stop", target)
        ));
    }
    Ok(done)
}

pub fn restart(args: TargetArgs, ui: Ui) -> AppResult<()> {
    for target in vms::resolve(args.name.as_deref(), args.all)? {
        if !stop_one(&target, StopMode::Graceful, ui)? {
            return Err(AppError::Other(anyhow::anyhow!(
                "{} did not stop, so it was not restarted",
                target.vm_name
            )));
        }
        start_one(&target, ui, true)?;
    }
    Ok(())
}

//Stops every VM this program built, to get the memory back.
pub fn poweroff(ui: Ui) -> AppResult<()> {
    let targets = vms::resolve(None, true)?;
    let mut stopped = 0;
    for target in &targets {
        let hv = hypervisor_for(target);
        if matches!(
            hv.state(&target.vm_name)?,
            VmState::Running | VmState::Paused | VmState::Busy(_)
        ) {
            stop_one(target, StopMode::Graceful, ui)?;
            stopped += 1;
        }
    }
    if stopped == 0 {
        ui.done("no VMs were running");
    }
    Ok(())
}

pub fn remove(args: RemoveArgs, ui: Ui) -> AppResult<()> {
    let targets = vms::resolve(args.target.name.as_deref(), args.target.all)?;
    let names: Vec<&str> = targets.iter().map(|t| t.vm_name.as_str()).collect();
    let what = if args.delete_disks {
        "and delete its disks"
    } else {
        "(its disks are kept)"
    };
    confirm(ui, &format!("Remove {} {what}?", names.join(", ")))?;
    for target in &targets {
        remove_one(target, args.delete_disks, ui)?;
    }
    Ok(())
}

fn remove_one(target: &Target, delete_disks: bool, ui: Ui) -> AppResult<()> {
    let hv: Box<dyn Hypervisor> = hypervisor_for(target);
    let name = &target.vm_name;
    let vm_dir = match &target.record {
        Some(record) => record.vm_dir.clone(),
        None => crate::utilities::paths::user_path("vms")?.join(name),
    };
    let state = hv.state(name)?;
    if state == VmState::Missing && target.record.is_none() {
        ui.done(&format!("there is no VM named '{name}'; nothing to remove"));
        return Ok(());
    }
    hv.remove(name, &vm_dir, delete_disks)?;
    Record::delete(name)?;
    let _ = keys::forget_host(&[&target.hostname]);
    if delete_disks {
        ui.done(&format!("removed {name} and deleted its disks"));
    } else {
        ui.done_with_note(
            &format!("removed {name}"),
            &format!(
                "disks kept in {}",
                crate::utilities::paths::display_path(&vm_dir)
            ),
        );
    }
    Ok(())
}
