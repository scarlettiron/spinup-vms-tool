//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Every command, option and alias. clap turns these definitions into the parser,
//the help pages and the shell completions, so they are the one source of truth
//for the command line. The doc comments (///) become the help text.

use clap::{ArgAction, Args, Parser, Subcommand};
use clap_complete::Shell;

use crate::brand;

#[derive(Debug, Parser)]
#[command(
    name = brand::NAME,
    bin_name = brand::NAME,
    version = brand::LONG_VERSION,
    about = brand::TAGLINE,
    //-v, -V and --version are defined below, so turn off clap's own flag.
    disable_version_flag = true,
    //Plain `vmynt` prints the help.
    arg_required_else_help = true,
)]
pub struct Cli {
    /// Print the version
    #[arg(short = 'v', visible_short_alias = 'V', long = "version", action = ArgAction::Version)]
    pub version: Option<bool>,

    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

//Options every command accepts, before or after the command name.
#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// More detail, and the full cause chain on errors
    #[arg(long, global = true, display_order = 900)]
    pub verbose: bool,

    /// Print errors only
    #[arg(
        short,
        long,
        global = true,
        conflicts_with = "verbose",
        display_order = 900
    )]
    pub quiet: bool,

    /// Machine-readable output (list, status, config, version, blueprints...)
    #[arg(long, global = true, display_order = 900)]
    pub json: bool,

    /// Plain output without colors (NO_COLOR is honoured too)
    #[arg(long, global = true, display_order = 900)]
    pub no_color: bool,

    /// Answer yes to every confirmation, and never prompt
    #[arg(short = 'y', long, global = true, display_order = 900)]
    pub yes: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create .vmynt/ config in this folder
    Init(InitArgs),

    /// Show the merged config; --validate to check it
    Config(ConfigArgs),

    /// Build the VM from config
    #[command(visible_aliases = ["new", "build"])]
    Create(CreateArgs),

    /// Start or resume; never provisions
    #[command(visible_alias = "up")]
    Start(TargetArgs),

    /// Shut down, keeping the disks
    #[command(visible_aliases = ["halt", "down"])]
    Stop(StopArgs),

    /// Stop, then start
    Restart(TargetArgs),

    /// Stop every VM on this computer
    Poweroff,

    /// Delete the VM; --delete-disks removes its disks too
    #[command(visible_aliases = ["rm", "destroy"])]
    Remove(RemoveArgs),

    /// Open the desktop; --console for the raw console
    #[command(visible_aliases = ["desktop", "console"])]
    Open(OpenArgs),

    /// Open a shell in the VM
    Ssh(NameArgs),

    /// Run one command in the VM
    Exec(ExecArgs),

    /// Show bootstrap and post-start logs
    Logs(LogsArgs),

    /// Every VM on this computer, with status and URLs
    #[command(visible_alias = "ls")]
    List(ListArgs),

    /// Full detail for one VM
    #[command(visible_aliases = ["info", "describe"])]
    Status(StatusArgs),

    /// Create, list, restore or delete snapshots
    Snapshot(SnapshotArgs),

    /// List blueprints, or make your own with `blueprints new`
    Blueprints(BlueprintsArgs),

    /// Make a password hash for the config
    Hash(HashArgs),

    /// Check this computer is ready to run VMs
    Doctor,

    /// Delete cached ISOs and leftover build files
    Clean(CleanArgs),

    /// Print shell completion for bash, zsh, fish, PowerShell or elvish
    Completion(CompletionArgs),

    /// Print the version
    Version,
}

#[derive(Debug, Args)]
#[command(
    after_help = "Run it in the folder you want the project in. It asks for anything not given as an option."
)]
pub struct InitArgs {
    /// Project name (default: this folder's name)
    #[arg(long)]
    pub name: Option<String>,

    /// Blueprint to use; with --from, the name of your new blueprint
    #[arg(long)]
    pub blueprint: Option<String>,

    /// Make your own blueprint by copying this one (use with --blueprint)
    #[arg(long, requires = "blueprint")]
    pub from: Option<String>,

    /// The VM's main user
    #[arg(long)]
    pub username: Option<String>,

    /// The user's full name
    #[arg(long, requires = "username")]
    pub real_name: Option<String>,

    /// The user's password as a SHA-512 crypt hash (make one with `hash`)
    #[arg(long, requires = "username")]
    pub password_hash: Option<String>,

    /// Don't set up a user now; fill vm.config.local.yaml in later
    #[arg(long, conflicts_with_all = ["username", "real_name", "password_hash"])]
    pub no_user: bool,

    /// Copy vm.config.yaml and vm.config.local.yaml from an existing checkout
    #[arg(long, value_name = "FOLDER", conflicts_with_all = ["blueprint", "from"])]
    pub import: Option<std::path::PathBuf>,

    /// Rewrite the example files in an existing .vmynt/ (your config is never touched)
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Check the config and report every problem
    #[arg(long)]
    pub validate: bool,

    /// Show the config for one entry of the Machines block
    #[arg(long)]
    pub machine: Option<String>,
}

//Which VM a command acts on. See docs/vmynt-design.md section 12.2.
#[derive(Debug, Args)]
pub struct TargetArgs {
    /// VM or machine name (default: the project in this folder)
    pub name: Option<String>,

    /// Every VM on this computer (for create: every machine in this project)
    #[arg(long, conflicts_with = "name")]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct NameArgs {
    /// VM or machine name (default: the project in this folder)
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Show what would be built without touching the hypervisor
    #[arg(long)]
    pub render: bool,

    /// Delete and rebuild the VM if it already exists
    #[arg(long, conflicts_with = "render")]
    pub force: bool,

    /// Don't take the clean-install snapshot after the install
    #[arg(long)]
    pub no_checkpoint: bool,
}

#[derive(Debug, Args)]
pub struct StopArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Save the VM's memory to disk (hibernate) instead of shutting down
    #[arg(long, conflicts_with = "force")]
    pub save: bool,

    /// Turn it off without a clean shutdown
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Delete the VM's disks as well
    #[arg(long)]
    pub delete_disks: bool,
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    /// VM or machine name (default: the project in this folder)
    pub name: Option<String>,

    /// Open the raw console instead of the desktop
    #[arg(long)]
    pub console: bool,

    /// Open straight away instead of waiting for the guest to be ready
    #[arg(long)]
    pub no_wait: bool,
}

#[derive(Debug, Args)]
pub struct ExecArgs {
    /// VM or machine name (default: the project in this folder)
    #[arg(short, long)]
    pub name: Option<String>,

    /// The command to run, after `--`
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
    /// VM or machine name (default: the project in this folder)
    pub name: Option<String>,

    /// Keep printing new lines as they arrive
    #[arg(short, long)]
    pub follow: bool,

    /// How many lines to show from the end
    #[arg(short = 'n', long, default_value_t = 100)]
    pub lines: u32,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Ask each running VM for its database and bootstrap state
    #[arg(long)]
    pub live: bool,

    /// Rebuild the project list from the VMs' hypervisor tags
    #[arg(long)]
    pub rescan: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Keep checking until bootstrap finishes
    #[arg(long)]
    pub follow: bool,

    /// Don't connect to the VM; host-side facts only
    #[arg(long)]
    pub no_ssh: bool,

    /// Also print the guest's bootstrap report (every step and its result)
    #[arg(long, conflicts_with = "no_ssh")]
    pub report: bool,
}

#[derive(Debug, Args)]
pub struct SnapshotArgs {
    #[command(subcommand)]
    pub action: SnapshotAction,
}

#[derive(Debug, Subcommand)]
pub enum SnapshotAction {
    /// Take a snapshot
    Create { label: String, name: Option<String> },
    /// List snapshots
    #[command(visible_alias = "ls")]
    List { name: Option<String> },
    /// Roll the VM back to a snapshot
    Restore { label: String, name: Option<String> },
    /// Delete a snapshot
    #[command(visible_alias = "rm")]
    Delete { label: String, name: Option<String> },
}

#[derive(Debug, Args)]
pub struct BlueprintsArgs {
    #[command(subcommand)]
    pub action: Option<BlueprintsAction>,
}

#[derive(Debug, Subcommand)]
pub enum BlueprintsAction {
    /// List the blueprints visible from here (the default)
    #[command(visible_alias = "ls")]
    List,
    /// Copy a blueprint into this project's .vmynt/blueprints/ to edit
    New {
        /// Name for your blueprint
        name: String,
        /// The blueprint to start from
        #[arg(long)]
        from: String,
    },
}

#[derive(Debug, Args)]
pub struct HashArgs {
    /// Read the password from standard input instead of prompting
    #[arg(long)]
    pub stdin: bool,
}

#[derive(Debug, Args)]
pub struct CleanArgs {
    /// Show what would be deleted, and delete nothing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// The shell to print completion for
    pub shell: Shell,
}
