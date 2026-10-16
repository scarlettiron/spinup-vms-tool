//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Everything the program prints goes through here, so --quiet, --json and
//--no-color behave the same in every command.
//
//anstream removes the colors automatically when the output is not a terminal,
//or when NO_COLOR is set, and makes them work on older Windows consoles.

use std::io::IsTerminal;

use anstream::{eprint, eprintln, println};
use anstyle::{AnsiColor, Style};

const GREEN: Style = AnsiColor::Green.on_default().bold();
const YELLOW: Style = AnsiColor::Yellow.on_default().bold();
const RED: Style = AnsiColor::Red.on_default().bold();
const DIM: Style = Style::new().dimmed();
const BOLD: Style = Style::new().bold();

//How much to print, chosen by the global flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ui {
    pub quiet: bool,
    pub json: bool,
    pub verbose: bool,
    //--yes: answer "yes" to every confirmation.
    pub assume_yes: bool,
}

impl Ui {
    //True when prompts can be shown: both ends are a terminal and the output is
    //meant for a person.
    pub fn interactive(&self) -> bool {
        !self.json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
    }

    //A finished step: "✔ wrote vm.config.yaml".
    pub fn done(&self, message: &str) {
        if !self.quiet && !self.json {
            println!("{GREEN}✔{GREEN:#} {message}");
        }
    }

    //A finished step with a note after it, dimmed.
    pub fn done_with_note(&self, message: &str, note: &str) {
        if !self.quiet && !self.json {
            println!("{GREEN}✔{GREEN:#} {message}  {DIM}{note}{DIM:#}");
        }
    }

    //Ordinary output for people.
    pub fn line(&self, message: &str) {
        if !self.quiet && !self.json {
            println!("{message}");
        }
    }

    //A heading line in bold.
    pub fn heading(&self, message: &str) {
        if !self.quiet && !self.json {
            println!("{BOLD}{message}{BOLD:#}");
        }
    }

    //A dimmed hint, like "Next: run vmseed create".
    pub fn hint(&self, message: &str) {
        if !self.quiet && !self.json {
            println!("{DIM}{message}{DIM:#}");
        }
    }

    //Extra detail, only with --verbose. Goes to stderr so it never mixes with
    //output a script is reading.
    pub fn detail(&self, message: &str) {
        if self.verbose {
            eprintln!("{DIM}{message}{DIM:#}");
        }
    }

    //A numbered step of a long command: "[3/7] Building the seed disk". Long
    //commands report on stderr, so --json output on stdout stays clean.
    pub fn step(&self, number: usize, total: usize, message: &str) {
        if !self.quiet {
            eprintln!("{BOLD}[{number}/{total}]{BOLD:#} {message}");
        }
    }

    //A dimmed line under a step, shown unless --quiet.
    pub fn info(&self, message: &str) {
        if !self.quiet {
            eprintln!("      {DIM}{message}{DIM:#}");
        }
    }

    //A line that rewrites itself (download progress). Only on a terminal; a log
    //file would fill with hundreds of lines.
    pub fn progress(&self, message: &str) {
        if !self.quiet && std::io::stderr().is_terminal() {
            eprint!("\r\x1b[2K      {DIM}{message}{DIM:#}");
        }
    }

    //Ends a progress line.
    pub fn progress_done(&self) {
        if !self.quiet && std::io::stderr().is_terminal() {
            eprint!("\r\x1b[2K");
        }
    }

    //Something worth knowing that did not stop the command.
    pub fn warn(&self, message: &str) {
        if !self.quiet {
            eprintln!("{YELLOW}warning:{YELLOW:#} {message}");
        }
    }

    //A command's machine-readable result, for --json.
    pub fn json(&self, value: &serde_json::Value) {
        let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_string());
        println!("{text}");
    }
}

//Prints an error. Errors always print, even with --quiet.
pub fn print_error(message: &str) {
    eprintln!("{RED}error:{RED:#} {message}");
}

//Colors for doctor's check results.
pub fn ok_mark() -> String {
    format!("{GREEN}✔{GREEN:#}")
}

pub fn warn_mark() -> String {
    format!("{YELLOW}!{YELLOW:#}")
}

pub fn fail_mark() -> String {
    format!("{RED}✖{RED:#}")
}

//Turns colors off for the whole program, for --no-color.
pub fn disable_color() {
    anstream::ColorChoice::Never.write_global();
}
