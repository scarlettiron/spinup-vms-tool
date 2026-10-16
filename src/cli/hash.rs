//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt hash`: turns a password into a SHA-512 crypt hash for passwordHash in
//vm.config.local.yaml, so the plain password never has to be written down.

use std::io::Read;

use anstream::println;
use anyhow::Context;

use super::args::HashArgs;
use crate::errors::{AppError, AppResult};
use crate::passwd;
use crate::utilities::ui::Ui;

pub fn run(args: HashArgs, ui: Ui) -> AppResult<()> {
    let password = if args.stdin {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("could not read the password from standard input")?;
        //Drop the newline `echo` adds, and nothing else.
        text.strip_suffix("\r\n")
            .or_else(|| text.strip_suffix('\n'))
            .unwrap_or(&text)
            .to_string()
    } else if ui.interactive() {
        inquire::Password::new("Password:")
            .with_display_mode(inquire::PasswordDisplayMode::Masked)
            .with_custom_confirmation_message("Again:")
            .with_custom_confirmation_error_message("The two entries don't match.")
            .prompt()?
    } else {
        return Err(AppError::Usage(
            "no terminal to ask for a password; pipe it in with --stdin".to_string(),
        ));
    };

    if password.is_empty() {
        return Err(AppError::Usage("the password is empty".to_string()));
    }

    let hash = passwd::hash_password(&password)?;
    if ui.json {
        ui.json(&serde_json::json!({ "passwordHash": hash }));
    } else {
        //The hash itself always prints, even with --quiet; it is the whole point.
        println!("{hash}");
        //The hint only goes to a person, so `hash --stdin` piped into a script
        //produces exactly one line.
        if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
            return Ok(());
        }
        ui.hint("Paste it into .vmynt/vm.config.local.yaml as:  passwordHash: '<the line above>'");
    }
    Ok(())
}
