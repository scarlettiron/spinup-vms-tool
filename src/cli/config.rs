//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt config`: shows the project's merged config, with secrets hidden, and
//`--validate` runs every check `create` runs, and reports all of them.

use anstream::print;
use serde_norway::{Mapping, Value};

use super::args::ConfigArgs;
use crate::blueprint;
use crate::brand;
use crate::config::{self, ProjectConfig};
use crate::constants::files;
use crate::errors::{AppError, AppResult};
use crate::passwd;
use crate::utilities::{names, paths, ui::Ui};

//Settings whose values are never printed. Matched against the last part of the
//key, ignoring case.
const SECRET_KEYS: &[&str] = &[
    "Password",
    "PasswordHash",
    "Token",
    "AdminPassword",
    "ActivationKey",
];
const MASK: &str = "********";

pub fn run(args: ConfigArgs, ui: Ui) -> AppResult<()> {
    let Some(root) = paths::current_project_root()? else {
        return Err(AppError::NotFound(format!(
            "no {} project in this folder or above it. Run `{} init` to make one.",
            brand::DISPLAY_NAME,
            brand::NAME
        )));
    };
    ui.detail(&format!("project folder: {}", paths::display_path(&root)));
    let project = ProjectConfig::load(&root, args.machine.as_deref())?;

    if args.validate {
        return validate(&project, ui);
    }

    let mut shown = project.settings.clone();
    mask_secrets(&mut shown);

    if ui.json {
        let value = serde_json::to_value(&shown).map_err(anyhow::Error::from)?;
        ui.json(&serde_json::json!({
            "project": root,
            "machine": project.machine,
            "machines": project.machines,
            "vmName": project.names().vm_name,
            "hostname": project.names().hostname,
            "settings": value,
        }));
        return Ok(());
    }

    ui.hint(&format!(
        "# {} merged with {}{}. Secrets are hidden.",
        files::CONFIG_FILE,
        files::LOCAL_CONFIG_FILE,
        project
            .machine
            .as_deref()
            .map(|machine| format!(" and machine '{machine}'"))
            .unwrap_or_default()
    ));
    ui.hint(&format!(
        "# VM name: {}   hostname: {}",
        project.names().vm_name,
        project.names().hostname
    ));
    if !ui.quiet {
        let text = serde_norway::to_string(&shown).map_err(anyhow::Error::from)?;
        print!("{text}");
    }
    if !project.machines.is_empty() {
        ui.hint(&format!(
            "# Machines: {}  (show one with --machine <name>)",
            project.machines.join(", ")
        ));
    }
    Ok(())
}

fn validate(project: &ProjectConfig, ui: Ui) -> AppResult<()> {
    let mut problems: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let project_name = project.project_name();
    let blueprint_name = project.blueprint();

    if let Err(message) = names::check_project_name(&project_name) {
        problems.push(format!("projectName '{project_name}': {message}"));
    }
    if blueprint::find(&blueprint_name, Some(&project.project_root))?.is_none() {
        problems.push(format!(
            "blueprint '{blueprint_name}' does not exist. Run `{} blueprints` to see them.",
            brand::NAME
        ));
    }
    let hostname = project.names().hostname;
    if hostname.len() > crate::constants::config_keys::MAX_HOSTNAME_LENGTH {
        problems.push(format!(
            "hostname '{hostname}' is longer than 63 characters; shorten projectName"
        ));
    }

    //A password belongs in the local file as a hash. A plain one in the shared
    //file would be committed and readable by everyone with the repository.
    let shared_path = project
        .project_root
        .join(brand::DIR)
        .join(files::CONFIG_FILE);
    let shared = config::expand_aliases(config::load_layer(&shared_path)?, files::CONFIG_FILE)?;
    if config::get_path(&shared, "Guest.PrimaryUser.Password").is_some() {
        warnings.push(format!(
            "a plain `password` is in {}, which is committed. Move it to {} as a passwordHash (`{} hash` makes one).",
            files::CONFIG_FILE,
            files::LOCAL_CONFIG_FILE,
            brand::NAME
        ));
    }
    let has_password = config::get_path(&project.settings, "Guest.PrimaryUser.Password").is_some();
    let hash = config::get_string(&project.settings, "Guest.PrimaryUser.PasswordHash");
    if has_password && hash.is_some() {
        problems.push("both password and passwordHash are set; keep only passwordHash".to_string());
    }
    if let Some(hash) = &hash
        && !passwd::looks_like_sha512_hash(hash)
    {
        problems.push(
            "passwordHash is not a SHA-512 crypt hash (it should start with $6$)".to_string(),
        );
    }
    if config::get_string(&project.settings, "Guest.PrimaryUser.Username").is_none() {
        warnings.push(format!(
            "no username is set yet; add one to {}",
            files::LOCAL_CONFIG_FILE
        ));
    }

    //The full checks `create` runs: blueprint, version, disks, database, files,
    //hooks, repos and keys.
    if problems.is_empty() {
        match crate::config::effective::Effective::load(
            &project.project_root,
            project.machine.as_deref(),
            serde_norway::Mapping::new(),
        ) {
            Ok(full) => {
                match crate::plans::check_all(&full) {
                    Ok(found) => warnings.extend(
                        found
                            .into_iter()
                            .filter(|w| !w.contains("a plain password")),
                    ),
                    Err(error) => problems.push(format!("{error:#}")),
                }
                if let Err(error) = crate::plans::check_user(&full)
                    && config::get_string(&project.settings, "Guest.PrimaryUser.Username").is_some()
                {
                    problems.push(format!("{error:#}"));
                }
            }
            Err(error) => problems.push(format!("{error:#}")),
        }
    }

    if ui.json {
        ui.json(&serde_json::json!({ "valid": problems.is_empty(), "problems": problems, "warnings": warnings }));
    } else {
        for warning in &warnings {
            ui.warn(warning);
        }
        for problem in &problems {
            crate::utilities::ui::print_error(problem);
        }
        if problems.is_empty() {
            ui.done("the config looks right");
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(AppError::Other(anyhow::anyhow!(
            "{} problem(s) found in the config",
            problems.len()
        )))
    }
}

//Replaces the value of every secret setting with asterisks.
fn mask_secrets(map: &mut Mapping) {
    for (key, value) in map.iter_mut() {
        let is_secret = key.as_str().is_some_and(|name| {
            SECRET_KEYS
                .iter()
                .any(|secret| secret.eq_ignore_ascii_case(name))
        });
        match value {
            Value::Mapping(inner) => mask_secrets(inner),
            Value::Null => {}
            Value::String(text) if text.is_empty() => {}
            other if is_secret => *other = Value::String(MASK.to_string()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_masked_at_any_depth() {
        let mut map: Mapping =
            serde_norway::from_str("Guest:\n  PrimaryUser:\n    PasswordHash: '$6$x$y'\n    Username: me\nGitHub:\n  Cli:\n    Token: abc\n")
                .unwrap();
        mask_secrets(&mut map);
        let text = serde_norway::to_string(&map).unwrap();
        assert!(!text.contains("$6$x$y"));
        assert!(!text.contains("abc"));
        assert!(text.contains("me"));
    }
}
