//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmseed open`: from a cold computer to a desktop in one command. Starts the VM
//if needed, waits until the guest can actually serve a desktop, then opens it.
//The waiting matters: on Hyper-V a window opened while the guest is still booting is stuck in a basic session with no clipboard.

use std::time::{Duration, Instant};

use super::args::OpenArgs;
use super::common::{again, existing_state, guest_target, hypervisor_for, minutes};
use crate::errors::AppResult;
use crate::guest;
use crate::hypervisor::VmState;
use crate::utilities::ui::Ui;
use crate::vms;

const READY_TIMEOUT: Duration = Duration::from_secs(240);

pub fn run(args: OpenArgs, ui: Ui) -> AppResult<()> {
    let target = vms::resolve_one(args.name.as_deref())?;
    let hv = hypervisor_for(&target);
    let name = &target.vm_name;
    let state = existing_state(hv.as_ref(), &target)?;

    if args.console {
        if state != VmState::Running {
            //vfkit can only show a window for a VM it starts with one.
            hv.open_console(name)?;
            if hv.state(name)? != VmState::Running {
                hv.start(name, true)?;
            }
        } else {
            hv.open_console(name)?;
        }
        ui.done(&format!("opened the console for {name}"));
        return Ok(());
    }

    if state != VmState::Running {
        hv.start(name, false)?;
        ui.done(&format!("started {name}"));
    }

    let start = Instant::now();
    let mut next_tick = Duration::from_secs(20);
    let mut ready = false;
    let mut address = None;
    if args.no_wait {
        ui.info("not waiting for the guest, as asked");
    } else {
        ui.info("waiting for the desktop");
        while start.elapsed() < READY_TIMEOUT {
            let ip = hv.ip(name).ok().flatten();
            address = guest::address(&target.hostname, ip.as_deref());
            if hv.desktop_ready(name, address.as_deref()) {
                ready = true;
                break;
            }
            if start.elapsed() >= next_tick {
                ui.info(&format!(
                    "still booting, {} elapsed",
                    minutes(start.elapsed())
                ));
                next_tick += Duration::from_secs(20);
            }
            std::thread::sleep(Duration::from_secs(3));
        }
        if ready {
            ui.info(&format!("guest is up after {}", minutes(start.elapsed())));
            //The display manager needs a moment after the network appears.
            std::thread::sleep(Duration::from_secs(5));
        } else {
            ui.warn(&format!(
                "the desktop was not ready within {}s; opening anyway. If it doesn't connect, wait and run `{}` again.",
                READY_TIMEOUT.as_secs(),
                again("open", &target)
            ));
        }
    }

    let user = target.user.clone().unwrap_or_default();
    let address = match address {
        Some(address) => address,
        None => guest_target(hv.as_ref(), &target)
            .map(|t| t.address)
            .unwrap_or(format!("{}.local", target.hostname)),
    };
    hv.open_desktop(name, &address, &user)?;
    ui.done(&format!("opened the desktop for {name}"));
    if !user.is_empty() {
        ui.line(&format!(
            "  Log in as {user}, with the password from your config."
        ));
    }
    ui.hint(&format!(
        "  Shut it down when you're done, to get the memory back: {}",
        again("stop", &target)
    ));
    Ok(())
}
