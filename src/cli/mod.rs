//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The commands. Each one lives in its own file; this file sends the parsed
//command line to the right one.

pub mod args;
pub mod help;

mod blueprints;
mod clean;
mod common;
mod completion;
mod config;
mod create;
mod doctor;
mod hash;
mod init;
mod lifecycle;
mod list;
mod open;
mod shell;
mod snapshot;
mod status;
mod version;

use args::Command;

use crate::errors::AppResult;
use crate::utilities::ui::Ui;

pub fn run(command: Command, ui: Ui) -> AppResult<()> {
    match command {
        Command::Init(args) => init::run(args, ui),
        Command::Config(args) => config::run(args, ui),
        Command::Create(args) => create::run(args, ui),
        Command::Start(args) => lifecycle::start(args, ui),
        Command::Stop(args) => lifecycle::stop(args, ui),
        Command::Restart(args) => lifecycle::restart(args, ui),
        Command::Poweroff => lifecycle::poweroff(ui),
        Command::Remove(args) => lifecycle::remove(args, ui),
        Command::Open(args) => open::run(args, ui),
        Command::Ssh(args) => shell::ssh(args, ui),
        Command::Exec(args) => shell::exec(args, ui),
        Command::Logs(args) => shell::logs(args, ui),
        Command::List(args) => list::run(args, ui),
        Command::Status(args) => status::run(args, ui),
        Command::Snapshot(args) => snapshot::run(args, ui),
        Command::Blueprints(args) => blueprints::run(args, ui),
        Command::Hash(args) => hash::run(args, ui),
        Command::Doctor(args) => doctor::run(args, ui),
        Command::Clean(args) => clean::run(args, ui),
        Command::Completion(args) => completion::run(args),
        Command::Version => version::run(ui),
    }
}
