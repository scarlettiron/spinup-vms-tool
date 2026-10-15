//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Rules for the names people type: project names, blueprint names and Linux
//usernames. Each check returns a sentence explaining what is wrong, ready to show.

use crate::constants::config_keys::MAX_HOSTNAME_LENGTH;

//A project name becomes part of a hostname, so letters, digits and hyphens,
//starting with a letter or digit. Same rule as the PowerShell tool.
pub fn check_project_name(name: &str) -> Result<(), String> {
    let mut letters = name.chars();
    let starts_well = letters
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric());
    let rest_ok = name
        .chars()
        .all(|letter| letter.is_ascii_alphanumeric() || letter == '-');
    if name.is_empty() {
        return Err("The project name can't be empty.".to_string());
    }
    if !starts_well || !rest_ok {
        return Err("Use letters, digits and hyphens, starting with a letter or digit. It becomes part of a hostname.".to_string());
    }
    Ok(())
}

//The VM and its hostname are "<project>-<blueprint>", which must fit in one DNS
//label.
pub fn check_hostname_length(project: &str, blueprint: &str) -> Result<(), String> {
    let length = project.len() + 1 + blueprint.len();
    if length > MAX_HOSTNAME_LENGTH {
        return Err(format!(
            "\"{project}-{blueprint}\" is {length} characters; hostnames stop at {MAX_HOSTNAME_LENGTH}. Use a shorter project name."
        ));
    }
    Ok(())
}

//Blueprint names are folder names: lowercase letters, digits and hyphens.
pub fn check_blueprint_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && name
            .chars()
            .all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '-');
    if valid {
        Ok(())
    } else {
        Err(
            "Use lowercase letters, digits and hyphens, starting with a letter or digit."
                .to_string(),
        )
    }
}

//A Linux username: starts with a lowercase letter or underscore, then lowercase
//letters, digits, underscores or hyphens, at most 32 characters.
pub fn check_username(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && name.chars().all(|letter| {
            letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '_' || letter == '-'
        });
    if valid {
        Ok(())
    } else {
        Err("Use lowercase letters, digits, '_' or '-', starting with a letter, at most 32 characters.".to_string())
    }
}

//Turns any folder name into a usable project name: lowercase, anything else
//becomes a hyphen, no hyphens at the ends. Falls back to "dev".
pub fn project_name_from_folder(folder: &str) -> String {
    let mut name = String::new();
    for letter in folder.to_lowercase().chars() {
        if letter.is_ascii_alphanumeric() {
            name.push(letter);
        } else if !name.ends_with('-') {
            name.push('-');
        }
    }
    let trimmed = name.trim_matches('-').to_string();
    if trimmed.is_empty() {
        crate::constants::config_keys::DEFAULT_PROJECT_NAME.to_string()
    } else {
        trimmed
    }
}

//A snapshot label becomes a folder name on macOS and a checkpoint name on
//Hyper-V, so it keeps to letters, digits, dots, dashes and underscores.
pub fn check_snapshot_label(label: &str) -> anyhow::Result<()> {
    let ok = !label.is_empty()
        && label.len() <= 64
        && !label.starts_with('.')
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok {
        anyhow::bail!(
            "snapshot name '{label}' must be 1 to 64 letters, digits, dots, dashes or underscores"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_names() {
        assert!(check_project_name("acme").is_ok());
        assert!(check_project_name("Acme-2").is_ok());
        assert!(check_project_name("-acme").is_err());
        assert!(check_project_name("al con").is_err());
        assert!(check_project_name("").is_err());
    }

    #[test]
    fn hostname_length() {
        assert!(check_hostname_length("acme", "redhat-rocky-workstation").is_ok());
        assert!(check_hostname_length(&"a".repeat(50), "redhat-rocky-workstation").is_err());
    }

    #[test]
    fn usernames() {
        assert!(check_username("wxs").is_ok());
        assert!(check_username("Wxs").is_err());
        assert!(check_username("1abc").is_err());
    }

    #[test]
    fn folder_names_become_project_names() {
        assert_eq!(project_name_from_folder("My Project_2"), "my-project-2");
        assert_eq!(project_name_from_folder("___"), "dev");
    }
}
