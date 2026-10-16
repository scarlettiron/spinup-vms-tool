//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Where things live on disk: the per-user folder, the project folder, and
//programs on the PATH.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::brand;
use crate::constants::files;

//The person's home folder.
pub fn home_dir() -> Result<PathBuf> {
    std::env::home_dir().ok_or_else(|| anyhow!("could not find your home folder"))
}

//The per-user folder, ~/.vmseed. The VMSEED_HOME environment variable moves it,
//which the tests use so they never touch the real one.
pub fn user_dir() -> Result<PathBuf> {
    if let Some(custom) = std::env::var_os(brand::env_var("HOME"))
        && !custom.is_empty()
    {
        return Ok(PathBuf::from(custom));
    }
    Ok(home_dir()?.join(brand::DIR))
}

//A file or folder inside the per-user folder.
pub fn user_path(name: &str) -> Result<PathBuf> {
    Ok(user_dir()?.join(name))
}

//Walks up from `start` looking for a .vmseed folder that holds a config file, the
//way git looks for .git. Returns the project folder (the parent of .vmseed).
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join(brand::DIR).join(files::CONFIG_FILE).is_file() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

//The project folder for the current directory, if there is one.
pub fn current_project_root() -> Result<Option<PathBuf>> {
    let cwd = std::env::current_dir().context("could not read the current folder")?;
    Ok(find_project_root(&cwd))
}

//Looks for a program on the PATH, the way a shell would. On Windows it also
//tries the extensions in PATHEXT, so "ssh" finds "ssh.exe".
pub fn find_program(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| ext.to_lowercase())
            .collect()
    } else {
        Vec::new()
    };

    for dir in std::env::split_paths(&path_var) {
        let plain = dir.join(name);
        if plain.is_file() {
            return Some(plain);
        }
        for ext in &extensions {
            let with_ext = dir.join(format!("{name}{ext}"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}

//Shortens a path under the home folder to start with "~", for display only.
pub fn display_path(path: &Path) -> String {
    if let Ok(home) = home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        if rest.as_os_str().is_empty() {
            return "~".to_string();
        }
        let separator = std::path::MAIN_SEPARATOR;
        return format!("~{separator}{}", rest.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_project_root_from_a_nested_folder() {
        let root = std::env::temp_dir().join(format!("vmseed-paths-test-{}", std::process::id()));
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(root.join(brand::DIR)).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            root.join(brand::DIR).join(files::CONFIG_FILE),
            "projectName: x\n",
        )
        .unwrap();

        assert_eq!(find_project_root(&nested), Some(root.clone()));

        std::fs::remove_dir_all(&root).unwrap();
    }
}
