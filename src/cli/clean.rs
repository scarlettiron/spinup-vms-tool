//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt clean`: deletes the downloaded ISO cache and leftover temporary files in
//~/.vmynt. It never touches VMs, their disks, or any project.

use std::path::{Path, PathBuf};

use super::args::CleanArgs;
use crate::constants::files;
use crate::errors::{AppError, AppResult};
use crate::utilities::{paths, ui::Ui};

pub fn run(args: CleanArgs, ui: Ui) -> AppResult<()> {
    let targets: Vec<PathBuf> = [files::ISO_CACHE_DIR, files::TEMP_DIR]
        .iter()
        .map(|name| paths::user_path(name))
        .collect::<Result<_, _>>()?;
    let existing: Vec<(PathBuf, u64)> = targets
        .into_iter()
        .filter(|path| path.exists())
        .map(|path| {
            let size = folder_size(&path);
            (path, size)
        })
        .collect();

    if existing.is_empty() {
        ui.done("nothing to clean");
        return Ok(());
    }

    let total: u64 = existing.iter().map(|(_, size)| size).sum();
    for (path, size) in &existing {
        ui.line(&format!(
            "  {}  {}",
            paths::display_path(path),
            human_size(*size)
        ));
    }
    if args.dry_run {
        ui.hint(&format!(
            "Would free {}. Nothing was deleted (--dry-run).",
            human_size(total)
        ));
        return Ok(());
    }

    if !ui.assume_yes {
        if !ui.interactive() {
            return Err(AppError::Usage(
                "add --yes to delete without a prompt".to_string(),
            ));
        }
        let confirmed = inquire::Confirm::new(&format!("Delete these ({})?", human_size(total)))
            .with_default(false)
            .prompt()?;
        if !confirmed {
            return Err(AppError::Cancelled);
        }
    }

    for (path, _) in &existing {
        std::fs::remove_dir_all(path)
            .map_err(|error| anyhow::anyhow!("could not delete {}: {error}", path.display()))?;
        ui.done(&format!("deleted {}", paths::display_path(path)));
    }
    Ok(())
}

//The total size of the files in a folder, counting subfolders.
fn folder_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_size(&entry.path()),
            Ok(_) => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::human_size;

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
