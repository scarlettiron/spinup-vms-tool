//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Writing files safely.

use std::path::Path;

use anyhow::{Context, Result};

//Writes a file by writing a temporary copy beside it and then renaming it into
//place. A crash halfway through leaves the old file, never half of the new one.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()));
    std::fs::write(&temporary, contents)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

//Like write_atomic, but readable only by the owner on Linux and macOS. Used for
//files holding a password hash.
pub fn write_private(path: &Path, contents: &str) -> Result<()> {
    write_atomic(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("could not restrict access to {}", path.display()))?;
    }
    Ok(())
}
