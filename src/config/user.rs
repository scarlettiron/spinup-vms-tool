//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The per-user settings file, ~/.vmseed/config.yaml. Today it holds the default
//identity `init` offers for new projects, so people type their username and
//password once rather than in every project.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::constants::files;
use crate::utilities::{files as file_utils, paths};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
}

//The guest's main user. Only the password's hash is ever stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub username: String,
    #[serde(default)]
    pub real_name: String,
    pub password_hash: String,
}

impl UserConfig {
    //Reads the file, or returns an empty config when there is none yet.
    pub fn load() -> Result<Self> {
        let path = paths::user_path(files::USER_CONFIG_FILE)?;
        if !path.is_file() {
            return Ok(UserConfig::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        if text.trim().is_empty() {
            return Ok(UserConfig::default());
        }
        serde_norway::from_str(&text).with_context(|| format!("{} is not valid", path.display()))
    }

    //Saves the file, readable only by its owner because it holds a password hash.
    pub fn save(&self) -> Result<()> {
        let path = paths::user_path(files::USER_CONFIG_FILE)?;
        let header = format!(
            "# {} settings for this computer. Written by `{} init`; safe to edit.\n",
            crate::brand::DISPLAY_NAME,
            crate::brand::NAME
        );
        let body = serde_norway::to_string(self).context("could not write the user config")?;
        file_utils::write_private(&path, &(header + &body))
    }
}
