//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The list of projects on this computer, ~/.vmseed/projects.yaml. `init` adds a
//project, and `list` reads it, so VMs can be found from any folder the way ddev
//finds its projects.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::constants::files;
use crate::utilities::{files as file_utils, paths};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Registry {
    //Keyed by project name. VM names are derived from project names, so two
    //projects with one name would fight over one VM and one hostname.
    #[serde(default)]
    pub projects: BTreeMap<String, ProjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectEntry {
    pub path: PathBuf,
    pub blueprint: String,
    //When `init` registered it, as an RFC 3339 timestamp.
    pub registered: String,
}

impl Registry {
    pub fn load() -> Result<Self> {
        let path = paths::user_path(files::REGISTRY_FILE)?;
        if !path.is_file() {
            return Ok(Registry::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        if text.trim().is_empty() {
            return Ok(Registry::default());
        }
        serde_norway::from_str(&text).with_context(|| format!("{} is not valid", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::user_path(files::REGISTRY_FILE)?;
        let header = format!(
            "# Projects {} knows about on this computer. Written by `{} init`.\n",
            crate::brand::DISPLAY_NAME,
            crate::brand::NAME
        );
        let body = serde_norway::to_string(self).context("could not write the project list")?;
        file_utils::write_atomic(&path, &(header + &body))
    }

    //The folder a name is registered to, when that folder still exists and is
    //not `path`. Used to stop two live projects sharing a name.
    pub fn conflicting_path(&self, name: &str, path: &Path) -> Option<PathBuf> {
        let entry = self.projects.get(name)?;
        let same = entry.path == path
            || std::fs::canonicalize(&entry.path).ok() == std::fs::canonicalize(path).ok();
        if same || !entry.path.exists() {
            None
        } else {
            Some(entry.path.clone())
        }
    }

    pub fn register(&mut self, name: &str, path: &Path, blueprint: &str) {
        let registered =
            humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string();
        self.projects.insert(
            name.to_string(),
            ProjectEntry {
                path: path.to_path_buf(),
                blueprint: blueprint.to_string(),
                registered,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_is_not_a_conflict() {
        let mut registry = Registry::default();
        registry.register(
            "acme",
            Path::new("/definitely/not/here"),
            "redhat-rocky-workstation",
        );
        assert_eq!(
            registry.conflicting_path("acme", Path::new("/somewhere/else")),
            None
        );
    }

    #[test]
    fn a_live_folder_with_the_same_name_is_a_conflict() {
        let mut registry = Registry::default();
        let live = std::env::temp_dir();
        registry.register("acme", &live, "redhat-rocky-workstation");
        assert_eq!(
            registry.conflicting_path("acme", Path::new("/somewhere/else")),
            Some(live.clone())
        );
        assert_eq!(registry.conflicting_path("acme", &live), None);
    }
}
