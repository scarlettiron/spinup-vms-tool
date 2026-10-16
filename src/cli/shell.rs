//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//ssh, exec and logs: reaching into a running guest with the computer's own
//ssh program, using Vmseed's key (added to every guest it builds).

use super::args::{ExecArgs, LogsArgs, NameArgs};
use super::common::{again, existing_state, guest_target, hypervisor_for};
use crate::errors::{AppError, AppResult};
use crate::guest;
use crate::hypervisor::VmState;
use crate::utilities::ui::Ui;
use crate::vms;

fn running_target(name: Option<&str>) -> AppResult<(vms::Target, guest::Target)> {
    let target = vms::resolve_one(name)?;
    let hv = hypervisor_for(&target);
    if existing_state(hv.as_ref(), &target)? != VmState::Running {
        return Err(AppError::Usage(format!(
            "{} is not running. Start it with `{}`.",
            target.vm_name,
            again("start", &target)
        )));
    }
    let guest = guest_target(hv.as_ref(), &target)?;
    Ok((target, guest))
}

pub fn ssh(args: NameArgs, _ui: Ui) -> AppResult<()> {
    let (_, guest) = running_target(args.name.as_deref())?;
    let code = guest::interactive(&guest, &[])?;
    exit_with(code)
}

pub fn exec(args: ExecArgs, _ui: Ui) -> AppResult<()> {
    let (_, guest) = running_target(args.name.as_deref())?;
    let code = guest::interactive(&guest, &args.command)?;
    exit_with(code)
}

pub fn logs(args: LogsArgs, _ui: Ui) -> AppResult<()> {
    let (_, guest) = running_target(args.name.as_deref())?;
    let command = guest::logs_command(args.follow, args.lines);
    let code = guest::interactive(&guest, &[command])?;
    exit_with(code)
}

//The remote command's exit code becomes ours, as with ssh itself. 255 is ssh's
//own "could not connect".
fn exit_with(code: i32) -> AppResult<()> {
    match code {
        0 => Ok(()),
        255 => Err(AppError::Other(anyhow::anyhow!(
            "could not connect over ssh. The guest may still be booting, or your key is not in it yet (bootstrap adds \
             it during the install)."
        ))),
        other => std::process::exit(other),
    }
}
