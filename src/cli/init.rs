//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt init`: turns the current folder into a project. It asks for a project
//name, a blueprint (or makes a custom one), and the VM's user, then writes the
//.vmynt/ folder and registers the project so `list` can find it.
//
//Every question has an option, so it also runs unattended:
//  vmynt init --name acme --blueprint redhat-rocky-workstation \
//             --username wxs --password-hash '$6$...' --yes

use std::path::{Path, PathBuf};

use anyhow::Context;
use inquire::validator::Validation;

use super::args::InitArgs;
use crate::blueprint;
use crate::brand;
use crate::config::{
    ProjectConfig,
    user::{Identity, UserConfig},
};
use crate::constants::files;
use crate::errors::{AppError, AppResult};
use crate::passwd;
use crate::registry::Registry;
use crate::utilities::{files as file_utils, names, paths, template::render_with_brand, ui::Ui};

//The templates, compiled into the program.
const SHARED_TEMPLATE: &str = include_str!("../../assets/templates/vm.config.yaml.tmpl");
const LOCAL_TEMPLATE: &str = include_str!("../../assets/templates/vm.config.local.yaml.tmpl");
const GITIGNORE_TEMPLATE: &str = include_str!("../../assets/templates/gitignore.tmpl");
const SSH_README_TEMPLATE: &str = include_str!("../../assets/templates/ssh_keys_README.md.tmpl");
//The full reference files, shared with the PowerShell tool at the repository root.
const SHARED_EXAMPLE: &str = include_str!("../../vm.config.example.yaml");
const LOCAL_EXAMPLE: &str = include_str!("../../vm.config.local.example.yaml");

const CUSTOM_CHOICE: &str = "Create my own…";

//Which blueprint the project uses.
enum BlueprintChoice {
    Existing(String),
    //A copy of `from`, made into the project as `name`.
    Custom { name: String, from: String },
}

impl BlueprintChoice {
    fn name(&self) -> &str {
        match self {
            BlueprintChoice::Existing(name) | BlueprintChoice::Custom { name, .. } => name,
        }
    }
}

//The VM's user, and whether to remember it for future projects.
struct IdentityChoice {
    identity: Option<Identity>,
    save_as_default: bool,
}

pub fn run(args: InitArgs, ui: Ui) -> AppResult<()> {
    let cwd = std::env::current_dir().context("could not read the current folder")?;
    let folder = cwd.join(brand::DIR);

    if folder.exists() {
        if !args.force {
            return Err(AppError::Usage(format!(
                "this folder already has {}/. Run `{} init --force` to refresh the example files (your config is never touched).",
                brand::DIR,
                brand::NAME
            )));
        }
        let written = write_support_files(&folder)?;
        for file in written {
            ui.done(&format!("rewrote {}", relative(&cwd, &file)));
        }
        return Ok(());
    }

    if let Some(source) = &args.import {
        return import(source, &cwd, ui);
    }

    //Prompts only when a person is there to answer and --yes was not given.
    let ask = ui.interactive() && !ui.assume_yes;

    let project_name = choose_project_name(&args, &cwd, ask)?;
    let blueprint_choice = choose_blueprint(&args, &cwd, ask)?;
    names::check_hostname_length(&project_name, blueprint_choice.name())
        .map_err(AppError::Usage)?;

    let mut registry = Registry::load()?;
    if let Some(other) = registry.conflicting_path(&project_name, &cwd) {
        return Err(AppError::Usage(format!(
            "the project name '{project_name}' is already used by {}. VM names and hostnames come from it, so pick another.",
            paths::display_path(&other)
        )));
    }

    let identity = choose_identity(&args, ask)?;

    //Everything is decided; now write.
    let mut written: Vec<(PathBuf, &str)> = Vec::new();
    std::fs::create_dir_all(&folder)
        .with_context(|| format!("could not create {}", folder.display()))?;

    let shared_path = folder.join(files::CONFIG_FILE);
    let shared = render_with_brand(
        SHARED_TEMPLATE,
        &[
            ("projectName", project_name.clone()),
            ("blueprint", blueprint_choice.name().to_string()),
        ],
    );
    file_utils::write_atomic(&shared_path, &shared)?;
    written.push((shared_path, "project settings — commit this"));

    let local_path = folder.join(files::LOCAL_CONFIG_FILE);
    let local = render_with_brand(
        LOCAL_TEMPLATE,
        &[("identity", identity_block(identity.identity.as_ref()))],
    );
    file_utils::write_private(&local_path, &local)?;
    written.push((local_path, "your user and password hash — git-ignored"));

    for path in write_support_files(&folder)? {
        let note = support_note(&path);
        written.push((path, note));
    }

    if let BlueprintChoice::Custom { name, from } = &blueprint_choice {
        let copied = blueprint::copy_to_project(from, name, &cwd)?;
        written.push((copied, "your blueprint — edit it freely"));
    }

    registry.register(&project_name, &cwd, blueprint_choice.name());
    registry.save()?;

    if identity.save_as_default
        && let Some(saved) = &identity.identity
    {
        let mut user_config = UserConfig::load()?;
        user_config.identity = Some(saved.clone());
        user_config.save()?;
    }

    report(
        &cwd,
        &project_name,
        blueprint_choice.name(),
        &written,
        identity.identity.is_none(),
        ui,
    );
    Ok(())
}

fn choose_project_name(args: &InitArgs, cwd: &Path, ask: bool) -> AppResult<String> {
    if let Some(name) = &args.name {
        names::check_project_name(name)
            .map_err(|message| AppError::Usage(format!("--name: {message}")))?;
        return Ok(name.clone());
    }
    let folder_name = cwd
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let default = names::project_name_from_folder(&folder_name);
    if !ask {
        return Ok(default);
    }
    let answer = inquire::Text::new("Project name:")
        .with_default(&default)
        .with_help_message("Becomes part of the VM's name and hostname")
        .with_validator(|input: &str| Ok(as_validation(names::check_project_name(input))))
        .prompt()?;
    Ok(answer)
}

fn choose_blueprint(args: &InitArgs, cwd: &Path, ask: bool) -> AppResult<BlueprintChoice> {
    let available = blueprint::list(Some(cwd))?;
    let exists = |name: &str| available.iter().any(|item| item.name == name);
    let unknown = |name: &str| {
        AppError::NotFound(format!(
            "there is no blueprint called '{name}'. Run `{} blueprints` to see them.",
            brand::NAME
        ))
    };

    //From options.
    match (&args.blueprint, &args.from) {
        (Some(name), Some(from)) => {
            names::check_blueprint_name(name)
                .map_err(|message| AppError::Usage(format!("--blueprint: {message}")))?;
            if exists(name) {
                return Err(AppError::Usage(format!(
                    "a blueprint called '{name}' already exists; choose a new name for yours"
                )));
            }
            if !exists(from) {
                return Err(unknown(from));
            }
            return Ok(BlueprintChoice::Custom {
                name: name.clone(),
                from: from.clone(),
            });
        }
        (Some(name), None) => {
            return if exists(name) {
                Ok(BlueprintChoice::Existing(name.clone()))
            } else {
                Err(unknown(name))
            };
        }
        _ => {}
    }
    if !ask {
        return Ok(BlueprintChoice::Existing(
            files::DEFAULT_BLUEPRINT.to_string(),
        ));
    }

    //From a menu.
    let mut labels: Vec<String> = available
        .iter()
        .map(|item| format!("{}  —  {}", item.name, item.description))
        .collect();
    labels.push(format!(
        "{CUSTOM_CHOICE}  —  copy a blueprint into {}/blueprints/ to edit",
        brand::DIR
    ));
    let start = available
        .iter()
        .position(|item| item.name == files::DEFAULT_BLUEPRINT)
        .unwrap_or(0);
    let picked = inquire::Select::new("Blueprint:", labels)
        .with_starting_cursor(start)
        .with_page_size(10)
        .raw_prompt()?;

    if picked.index < available.len() {
        return Ok(BlueprintChoice::Existing(
            available[picked.index].name.clone(),
        ));
    }

    let bases: Vec<String> = available.iter().map(|item| item.name.clone()).collect();
    let from = inquire::Select::new("Start from:", bases)
        .with_starting_cursor(start)
        .prompt()?;
    let taken: Vec<String> = available.iter().map(|item| item.name.clone()).collect();
    let name = inquire::Text::new("Name for your blueprint:")
        .with_default(&format!("my-{from}"))
        .with_validator(move |input: &str| {
            if taken.iter().any(|existing| existing == input) {
                return Ok(Validation::Invalid("That name is taken.".into()));
            }
            Ok(as_validation(names::check_blueprint_name(input)))
        })
        .prompt()?;
    Ok(BlueprintChoice::Custom { name, from })
}

fn choose_identity(args: &InitArgs, ask: bool) -> AppResult<IdentityChoice> {
    let none = IdentityChoice {
        identity: None,
        save_as_default: false,
    };
    if args.no_user {
        return Ok(none);
    }

    //From options.
    if let Some(username) = &args.username {
        names::check_username(username)
            .map_err(|message| AppError::Usage(format!("--username: {message}")))?;
        let real_name = match &args.real_name {
            Some(name) => name.clone(),
            None if ask => inquire::Text::new("Full name:").with_default("").prompt()?,
            None => String::new(),
        };
        let password_hash = match &args.password_hash {
            Some(hash) if passwd::looks_like_sha512_hash(hash) => hash.clone(),
            Some(_) => {
                return Err(AppError::Usage(format!(
                    "--password-hash must be a SHA-512 crypt hash starting with $6$. `{} hash` makes one.",
                    brand::NAME
                )));
            }
            None if ask => passwd::hash_password(&ask_password()?)?,
            None => {
                return Err(AppError::Usage(format!(
                    "--username needs --password-hash when nothing can be asked. `{} hash --stdin` makes one.",
                    brand::NAME
                )));
            }
        };
        return Ok(IdentityChoice {
            identity: Some(Identity {
                username: username.clone(),
                real_name,
                password_hash,
            }),
            save_as_default: false,
        });
    }

    //A user saved by an earlier init.
    let saved = UserConfig::load()?.identity;
    if let Some(saved) = saved {
        if !ask {
            return Ok(IdentityChoice {
                identity: Some(saved),
                save_as_default: false,
            });
        }
        let reuse = inquire::Confirm::new(&format!("Use your saved user '{}'?", saved.username))
            .with_default(true)
            .with_help_message("Saved by an earlier init on this computer")
            .prompt()?;
        if reuse {
            return Ok(IdentityChoice {
                identity: Some(saved),
                save_as_default: false,
            });
        }
    } else if !ask {
        return Err(AppError::Usage(format!(
            "no user given. Pass --username and --password-hash (`{} hash --stdin` makes one), or --no-user to add one later.",
            brand::NAME
        )));
    }

    //Ask for a new one.
    let suggested = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .map(|name| name.to_lowercase())
        .ok()
        .filter(|name| names::check_username(name).is_ok())
        .unwrap_or_default();
    let username = inquire::Text::new("Username:")
        .with_default(&suggested)
        .with_help_message("The VM's main user, a Linux account name")
        .with_validator(|input: &str| Ok(as_validation(names::check_username(input))))
        .prompt()?;
    let real_name = inquire::Text::new("Full name:").prompt()?;
    let password_hash = passwd::hash_password(&ask_password()?)?;
    let save_as_default =
        inquire::Confirm::new("Save this user as your default for future projects?")
            .with_default(true)
            .with_help_message("Only the password's hash is stored")
            .prompt()?;

    Ok(IdentityChoice {
        identity: Some(Identity {
            username,
            real_name,
            password_hash,
        }),
        save_as_default,
    })
}

fn ask_password() -> AppResult<String> {
    let password = inquire::Password::new("Password:")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .with_help_message("Stored only as a SHA-512 hash")
        .with_custom_confirmation_message("Again:")
        .with_custom_confirmation_error_message("The two entries don't match.")
        .with_validator(|input: &str| {
            Ok(if input.is_empty() {
                Validation::Invalid("The password can't be empty.".into())
            } else {
                Validation::Valid
            })
        })
        .prompt()?;
    Ok(password)
}

//The identity lines for vm.config.local.yaml, or a commented-out stub to fill
//in later.
fn identity_block(identity: Option<&Identity>) -> String {
    match identity {
        Some(identity) => format!(
            "# The VM's main user. Change the password with `{app} hash`.\nusername:       {}\nrealName:       {}\npasswordHash:   {}\n",
            identity.username,
            yaml_quote(&identity.real_name),
            yaml_quote(&identity.password_hash),
            app = brand::NAME,
        ),
        None => format!(
            "# The VM's main user. Make the password hash with `{app} hash`.\n# username:       you\n# realName:       Your Name\n# passwordHash:   '$6$...'\n",
            app = brand::NAME,
        ),
    }
}

//Single-quoted YAML text, the safe choice for anything a person typed.
fn yaml_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

//The files that never hold anyone's settings: the reference copies, the
//.gitignore and the ssh_keys README. --force rewrites only these.
fn write_support_files(folder: &Path) -> AppResult<Vec<PathBuf>> {
    let files_to_write = [
        (
            folder.join(files::EXAMPLE_CONFIG_FILE),
            example_text(SHARED_EXAMPLE),
        ),
        (
            folder.join(files::LOCAL_EXAMPLE_CONFIG_FILE),
            example_text(LOCAL_EXAMPLE),
        ),
        (
            folder.join(files::GITIGNORE_FILE),
            render_with_brand(GITIGNORE_TEMPLATE, &[]),
        ),
        (
            folder.join(files::SSH_KEYS_DIR).join(files::README_FILE),
            render_with_brand(SSH_README_TEMPLATE, &[]),
        ),
    ];
    let mut written = Vec::new();
    for (path, contents) in files_to_write {
        file_utils::write_atomic(&path, &contents)?;
        written.push(path);
    }
    Ok(written)
}

//The reference files at the repository root were written for the PowerShell
//tool. Point their commands and doc links at this program instead.
fn example_text(raw: &str) -> String {
    let header = render_with_brand(
        "# Reference copy written by `{{app}} init`: every setting, with what it does.\n# {{App}} does not read this file. Copy the lines you want into vm.config.yaml\n# or vm.config.local.yaml.\n#\n",
        &[],
    );
    let body = raw
        .replace(".\\vm.ps1 ", &format!("{} ", brand::NAME))
        .replace(
            "docs/configuration.md",
            &format!("{}configuration/", brand::SITE_URL),
        )
        .replace("docs/", &format!("{}/blob/main/docs/", brand::REPOSITORY));
    header + &body
}

fn support_note(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    match name.as_str() {
        files::EXAMPLE_CONFIG_FILE => "every option, documented",
        files::LOCAL_EXAMPLE_CONFIG_FILE => "every personal option, documented",
        files::GITIGNORE_FILE => "keeps your local file and keys out of git",
        _ => "drop .pub files in ssh_keys/",
    }
}

//`init --import <folder>`: reuse the config files of an existing checkout of the
//PowerShell tool (or another project).
fn import(source: &Path, cwd: &Path, ui: Ui) -> AppResult<()> {
    let candidates = [source.join(brand::DIR), source.to_path_buf()];
    let Some(from) = candidates
        .iter()
        .find(|dir| dir.join(files::CONFIG_FILE).is_file())
    else {
        return Err(AppError::NotFound(format!(
            "no {} in {}",
            files::CONFIG_FILE,
            source.display()
        )));
    };

    let folder = cwd.join(brand::DIR);
    std::fs::create_dir_all(&folder)
        .with_context(|| format!("could not create {}", folder.display()))?;
    let mut written: Vec<(PathBuf, &str)> = Vec::new();
    for (name, note) in [
        (files::CONFIG_FILE, "imported"),
        (files::LOCAL_CONFIG_FILE, "imported — git-ignored"),
    ] {
        let source_file = from.join(name);
        if source_file.is_file() {
            let target = folder.join(name);
            std::fs::copy(&source_file, &target)
                .with_context(|| format!("could not copy {}", source_file.display()))?;
            written.push((target, note));
        }
    }
    for path in write_support_files(&folder)? {
        let note = support_note(&path);
        written.push((path, note));
    }

    let config = ProjectConfig::load(cwd, None)?;
    let project_name = config.project_name();
    let mut registry = Registry::load()?;
    if let Some(other) = registry.conflicting_path(&project_name, cwd) {
        ui.warn(&format!(
            "the project name '{project_name}' is also used by {}; change projectName in {} before building",
            paths::display_path(&other),
            files::CONFIG_FILE
        ));
    } else {
        registry.register(&project_name, cwd, &config.blueprint());
        registry.save()?;
    }

    report(cwd, &project_name, &config.blueprint(), &written, false, ui);
    if crate::config::get_path(&config.settings, "Guest.PrimaryUser.Password").is_some() {
        ui.warn(&format!(
            "the imported config has a plain `password`. Replace it with a passwordHash in {} (`{} hash` makes one).",
            files::LOCAL_CONFIG_FILE,
            brand::NAME
        ));
    }
    Ok(())
}

fn report(
    cwd: &Path,
    project_name: &str,
    blueprint_name: &str,
    written: &[(PathBuf, &str)],
    no_user: bool,
    ui: Ui,
) {
    if ui.json {
        let files: Vec<String> = written
            .iter()
            .map(|(path, _)| path.display().to_string())
            .collect();
        ui.json(&serde_json::json!({
            "project": project_name,
            "path": cwd,
            "blueprint": blueprint_name,
            "files": files,
        }));
        return;
    }
    ui.line("");
    for (path, note) in written {
        ui.done_with_note(&relative(cwd, path), note);
    }
    ui.done_with_note(
        &format!("registered \"{project_name}\""),
        &paths::user_path(files::REGISTRY_FILE)
            .map(|path| format!("in {}", paths::display_path(&path)))
            .unwrap_or_default(),
    );
    ui.line("");
    if no_user {
        ui.hint(&format!(
            "Add your user to {}/{} before building.",
            brand::DIR,
            files::LOCAL_CONFIG_FILE
        ));
    }
    ui.hint(&format!(
        "Next: check it with `{app} config --validate`, then build the VM with `{app} create`.",
        app = brand::NAME
    ));
}

//"Valid" or "Invalid(message)" for a prompt, from one of the name checks.
fn as_validation(result: Result<(), String>) -> Validation {
    match result {
        Ok(()) => Validation::Valid,
        Err(message) => Validation::Invalid(message.into()),
    }
}

//A path relative to the project folder, for display.
fn relative(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .map(|rest| rest.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utilities::template::render;

    #[test]
    fn identity_lines_are_valid_yaml() {
        let identity = Identity {
            username: "wxs".to_string(),
            real_name: "O'Brien: Dev".to_string(),
            password_hash: "$6$salt$hash".to_string(),
        };
        let block = identity_block(Some(&identity));
        let parsed: serde_norway::Mapping = serde_norway::from_str(&block).unwrap();
        assert_eq!(
            parsed.get("realName").and_then(|value| value.as_str()),
            Some("O'Brien: Dev")
        );
        assert_eq!(
            parsed.get("passwordHash").and_then(|value| value.as_str()),
            Some("$6$salt$hash")
        );
    }

    #[test]
    fn templates_render_to_valid_config() {
        let shared = render_with_brand(
            SHARED_TEMPLATE,
            &[
                ("projectName", "acme".to_string()),
                ("blueprint", "redhat-rocky-workstation".to_string()),
            ],
        );
        assert!(!shared.contains("{{"), "every placeholder is filled");
        let layer: serde_norway::Mapping = serde_norway::from_str(&shared).unwrap();
        crate::config::expand_aliases(layer, "template")
            .expect("the template uses only known settings");

        let local = render(LOCAL_TEMPLATE, &[("identity", identity_block(None))]);
        let _: Option<serde_norway::Mapping> = serde_norway::from_str(&local).unwrap();
    }

    #[test]
    fn examples_point_at_this_program() {
        let text = example_text(SHARED_EXAMPLE);
        assert!(!text.contains(".\\vm.ps1 "));
        assert!(text.contains(&format!("{}configuration/", brand::SITE_URL)));
    }
}
