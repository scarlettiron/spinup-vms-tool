//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Where the program starts. It reads the command line, runs the command, and
//turns the result into an exit code. The design is in docs/vmseed-design.md.

mod blueprint;
mod brand;
mod cli;
mod config;
mod constants;
mod errors;
mod guest;
mod host_setup;
mod hypervisor;
mod media;
mod passwd;
mod plans;
mod platform;
mod registry;
mod seed;
mod utilities;
mod vms;

use clap::FromArgMatches;

use cli::args::Cli;
use constants::exit_codes;
use errors::AppError;
use utilities::ui::{self, Ui};

fn main() {
    //Plain `vmseed` shows the help and succeeds, like ddev, rather than exiting
    //with clap's "missing command" code.
    if std::env::args_os().len() == 1 {
        let _ = cli::help::command().print_help();
        std::process::exit(exit_codes::SUCCESS);
    }

    //clap prints help, --version and command line mistakes itself, and exits
    //with 0 or 2 as is conventional.
    let matches = cli::help::command().get_matches();
    let parsed = match Cli::from_arg_matches(&matches) {
        Ok(parsed) => parsed,
        Err(error) => error.exit(),
    };

    if parsed.global.no_color {
        ui::disable_color();
    }
    let ui = Ui {
        quiet: parsed.global.quiet,
        json: parsed.global.json,
        verbose: parsed.global.verbose,
        assume_yes: parsed.global.yes,
    };

    //Right after setup added the user to the libvirt group, this login doesn't
    //have it yet; this runs the command again with it (Linux only).
    host_setup::rerun_with_new_group();

    let code = match cli::run(parsed.command, ui) {
        Ok(()) => exit_codes::SUCCESS,
        Err(AppError::Cancelled) => exit_codes::CANCELLED,
        Err(error) => {
            report(&error, ui);
            error.exit_code()
        }
    };
    std::process::exit(code);
}

//Prints an error, with its causes when --verbose is on.
fn report(error: &AppError, ui: Ui) {
    ui::print_error(&error.to_string());
    if let AppError::Other(inner) = error {
        let causes: Vec<String> = inner.chain().skip(1).map(ToString::to_string).collect();
        if ui.verbose {
            for cause in causes {
                ui::print_error(&format!("  caused by: {cause}"));
            }
        } else if let Some(first) = causes.first() {
            //The first cause is usually the useful part ("permission denied").
            ui::print_error(&format!("  {first}"));
        }
    }
}
