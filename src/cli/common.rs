//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Helpers the VM commands share: picking the hypervisor, waiting for a state,
//asking for confirmation, and reaching the guest.

use std::time::{Duration, Instant};

use anyhow::Result;

use crate::errors::{AppError, AppResult};
use crate::guest;
use crate::hypervisor::{self, Hypervisor, VmState};
use crate::utilities::ui::Ui;
use crate::vms::Target;

//The hypervisor a VM was built with, or this computer's.
pub fn hypervisor_for(target: &Target) -> Box<dyn Hypervisor> {
    match &target.record {
        Some(record) if record.hypervisor != hypervisor::host().kind() => {
            hypervisor::for_kind(record.hypervisor)
        }
        _ => hypervisor::host(),
    }
}

//The VM's state, failing with "not found" when it doesn't exist.
pub fn existing_state(hv: &dyn Hypervisor, target: &Target) -> AppResult<VmState> {
    let state = hv.state(&target.vm_name)?;
    if state == VmState::Missing {
        return Err(AppError::NotFound(format!(
            "there is no VM named '{}' on this computer. Build it with `{} create`.",
            target.vm_name,
            crate::brand::NAME
        )));
    }
    Ok(state)
}

//Polls until the VM reaches a state for which `done` is true, printing a line
//now and then so a long wait never looks hung. Returns false on timeout.
pub fn wait_for(
    hv: &dyn Hypervisor,
    name: &str,
    timeout: Duration,
    every: Duration,
    ui: Ui,
    done: impl Fn(&VmState) -> bool,
    describe: impl Fn(Duration) -> String,
) -> Result<bool> {
    let start = Instant::now();
    let mut next_tick = every;
    while start.elapsed() < timeout {
        if done(&hv.state(name)?) {
            return Ok(true);
        }
        if start.elapsed() >= next_tick {
            ui.info(&describe(start.elapsed()));
            next_tick += every;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Ok(false)
}

//Asks before something destructive. --yes skips the question; without a
//terminal to ask on, the command refuses rather than guessing.
pub fn confirm(ui: Ui, question: &str) -> AppResult<()> {
    if ui.assume_yes {
        return Ok(());
    }
    if !ui.interactive() {
        return Err(AppError::Usage(format!(
            "{question} Add --yes to go ahead without a prompt."
        )));
    }
    if inquire::Confirm::new(question)
        .with_default(false)
        .prompt()?
    {
        Ok(())
    } else {
        Err(AppError::Cancelled)
    }
}

//Where to reach the guest over SSH: <hostname>.local, or the IP the hypervisor
//knows.
pub fn guest_target(hv: &dyn Hypervisor, target: &Target) -> AppResult<guest::Target> {
    let user = guest::require_user(target.user.clone(), &target.vm_name)?;
    let ip = hv.ip(&target.vm_name).ok().flatten();
    match guest::address(&target.hostname, ip.as_deref()) {
        Some(address) => Ok(guest::Target {
            user,
            address,
            alias: target.hostname.clone(),
        }),
        None => Err(AppError::Other(anyhow::anyhow!(
            "{}.local does not resolve and the hypervisor has no address for '{}' yet. Is it running? (`{} status`)",
            target.hostname,
            target.vm_name,
            crate::brand::NAME
        ))),
    }
}

//"12m 5s"
pub fn minutes(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs >= 60 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

//The command a person types to act on this VM again.
pub fn again(command: &str, target: &Target) -> String {
    let in_project = crate::utilities::paths::current_project_root()
        .ok()
        .flatten()
        .zip(target.project.clone())
        .is_some_and(|(here, theirs)| {
            std::fs::canonicalize(here).ok() == std::fs::canonicalize(theirs).ok()
        });
    let name = if in_project {
        target.machine.clone()
    } else {
        Some(target.vm_name.clone())
    };
    match name {
        Some(name) => format!("{} {command} {name}", crate::brand::NAME),
        None => format!("{} {command}", crate::brand::NAME),
    }
}
