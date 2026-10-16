//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt snapshot create|list|restore|delete`. Hyper-V checkpoints, libvirt
//disk snapshots, or APFS clones on macOS; the last two need the VM stopped,
//and the command says so. `create` takes one automatically after the install,
//called clean-install.

use super::args::{SnapshotAction, SnapshotArgs};
use super::common::{again, confirm, existing_state, hypervisor_for};
use crate::errors::AppResult;
use crate::utilities::{names, text, ui::Ui};
use crate::vms;

pub fn run(args: SnapshotArgs, ui: Ui) -> AppResult<()> {
    match args.action {
        SnapshotAction::Create { label, name } => {
            names::check_snapshot_label(&label)?;
            let target = vms::resolve_one(name.as_deref())?;
            let hv = hypervisor_for(&target);
            existing_state(hv.as_ref(), &target)?;
            hv.snapshot_create(&target.vm_name, &label)?;
            ui.done(&format!("took snapshot '{label}' of {}", target.vm_name));
            ui.hint(&format!(
                "  Roll back to it with: {}",
                again(&format!("snapshot restore {label}"), &target)
            ));
        }
        SnapshotAction::List { name } => {
            let target = vms::resolve_one(name.as_deref())?;
            let hv = hypervisor_for(&target);
            existing_state(hv.as_ref(), &target)?;
            let list = hv.snapshot_list(&target.vm_name)?;
            if ui.json {
                ui.json(&serde_json::to_value(&list).unwrap_or_default());
            } else if list.is_empty() {
                ui.line(&format!("{} has no snapshots", target.vm_name));
            } else {
                let mut table = vec![vec!["SNAPSHOT".to_string(), "TAKEN".to_string()]];
                for snapshot in list {
                    table.push(vec![
                        snapshot.name,
                        snapshot.created.unwrap_or_else(|| "—".to_string()),
                    ]);
                }
                ui.line(text::table(&table).trim_end());
            }
        }
        SnapshotAction::Restore { label, name } => {
            let target = vms::resolve_one(name.as_deref())?;
            let hv = hypervisor_for(&target);
            existing_state(hv.as_ref(), &target)?;
            confirm(
                ui,
                &format!(
                    "Roll {} back to '{label}'? Changes since then are lost.",
                    target.vm_name
                ),
            )?;
            hv.snapshot_restore(&target.vm_name, &label)?;
            ui.done(&format!("{} is back at '{label}'", target.vm_name));
            ui.hint(&format!("  Start it with: {}", again("start", &target)));
        }
        SnapshotAction::Delete { label, name } => {
            let target = vms::resolve_one(name.as_deref())?;
            let hv = hypervisor_for(&target);
            existing_state(hv.as_ref(), &target)?;
            confirm(
                ui,
                &format!("Delete snapshot '{label}' of {}?", target.vm_name),
            )?;
            hv.snapshot_delete(&target.vm_name, &label)?;
            ui.done(&format!("deleted snapshot '{label}'"));
        }
    }
    Ok(())
}
