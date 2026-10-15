//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt completion <shell>`: prints a script that teaches the shell the commands
//and options, so Tab completes them. Package managers call this at install time.

use super::args::CompletionArgs;
use super::help;
use crate::brand;
use crate::errors::AppResult;

pub fn run(args: CompletionArgs) -> AppResult<()> {
    let mut command = help::command();
    clap_complete::generate(
        args.shell,
        &mut command,
        brand::NAME,
        &mut std::io::stdout(),
    );
    Ok(())
}
