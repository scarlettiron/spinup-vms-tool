//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The top-level help page. clap lists commands in one block; this groups them
//(Project, Lifecycle, Use, ...) the way ddev does, using the table in
//constants/commands.rs. Each command's own help page is clap's default.

use std::fmt::Write as _;

use anstyle::Style;
use clap::CommandFactory;

use super::args::Cli;
use crate::brand;
use crate::constants::commands::COMMAND_GROUPS;

const HEADING: Style = Style::new().bold().underline();
const LITERAL: Style = Style::new().bold();

//The full clap command, with the grouped help page attached.
pub fn command() -> clap::Command {
    let command = Cli::command();
    let groups = grouped_commands(&command);
    let template = format!(
        "{{about-with-newline}}\n{HEADING}Usage:{HEADING:#} {LITERAL}{name}{LITERAL:#} <command> [name] [options]\n\n{groups}{HEADING}Global options:{HEADING:#}\n{{options}}{{after-help}}",
        name = brand::NAME,
    );
    let after = format!(
        "Run `{name} help <command>` or `{name} <command> -h` for a command's options.\nDocs: {site}",
        name = brand::NAME,
        site = brand::SITE_URL,
    );
    command.help_template(template).after_help(after)
}

//The "Project / Lifecycle / ..." blocks: each command, its summary, and its aliases.
fn grouped_commands(command: &clap::Command) -> String {
    let width = COMMAND_GROUPS
        .iter()
        .flat_map(|(_, names)| names.iter())
        .map(|name| name.len())
        .max()
        .unwrap_or(0)
        + 2;

    let mut text = String::new();
    for (group, names) in COMMAND_GROUPS {
        let _ = writeln!(text, "{HEADING}{group}:{HEADING:#}");
        for name in *names {
            let (summary, aliases) = describe(command, name);
            let alias_note = if aliases.is_empty() {
                String::new()
            } else {
                format!("  ({})", aliases.join(", "))
            };
            let _ = writeln!(
                text,
                "  {LITERAL}{name:<width$}{LITERAL:#}{summary}{alias_note}"
            );
        }
        text.push('\n');
    }
    text
}

//A command's one-line summary and its visible aliases. `help` is added by clap
//itself, so it is described here.
fn describe(command: &clap::Command, name: &str) -> (String, Vec<String>) {
    if name == "help" {
        return ("Help for any command".to_string(), Vec::new());
    }
    let Some(sub) = command.get_subcommands().find(|sub| sub.get_name() == name) else {
        return (String::new(), Vec::new());
    };
    let summary = sub.get_about().map(ToString::to_string).unwrap_or_default();
    let aliases = sub.get_visible_aliases().map(str::to_string).collect();
    (summary, aliases)
}

#[cfg(test)]
mod tests {
    use super::*;

    //Every command clap knows must appear in exactly one help group, and every
    //name in the groups must be a real command, so the two can't drift apart.
    #[test]
    fn help_groups_match_the_commands() {
        let command = Cli::command();
        let mut grouped: Vec<&str> = COMMAND_GROUPS
            .iter()
            .flat_map(|(_, names)| names.iter().copied())
            .collect();
        grouped.sort_unstable();
        let before = grouped.len();
        grouped.dedup();
        assert_eq!(before, grouped.len(), "a command is listed in two groups");

        let mut real: Vec<String> = command
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .collect();
        real.push("help".to_string());
        real.sort_unstable();
        assert_eq!(grouped, real.iter().map(String::as_str).collect::<Vec<_>>());
    }

    #[test]
    fn the_command_definition_is_valid() {
        command().debug_assert();
    }
}
