//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Gets this computer ready to run VMs, so nobody has to follow install steps by
//hand. `init` offers it for a new project, `create` when the hypervisor can't be
//reached, and `doctor --fix` on demand.
//
//On Linux QEMU, libvirt and the UEFI firmware come from the distribution's
//packages, the libvirt service has to be running, and the user has to be in the
//libvirt group; each command runs with sudo. On Windows Hyper-V has to be turned
//on and the user has to be in Hyper-V Administrators; both run in one PowerShell
//with administrator rights, so Windows asks once, and Windows then has to
//restart. On macOS vfkit comes with the Homebrew formula, so there is nothing to
//do here.
//
//Nothing runs without asking: the steps are listed first, and each command is
//printed before it runs. What no command can fix (virtualization turned off in
//the firmware, a Windows edition without Hyper-V) is explained instead.

#[cfg(any(target_os = "linux", windows))]
use inquire::Confirm;

#[cfg(any(target_os = "linux", windows))]
use crate::brand;
use crate::errors::{AppError, AppResult};
use crate::utilities::ui::Ui;

#[cfg(target_os = "linux")]
//Folders where distributions put the UEFI firmware QEMU boots guests with.
pub const FIRMWARE_DIRS: [&str; 5] = [
    "/usr/share/OVMF",
    "/usr/share/AAVMF",
    "/usr/share/edk2",
    "/usr/share/qemu/firmware",
    "/usr/share/edk2-ovmf",
];

#[cfg(target_os = "linux")]
//Members may use libvirt's system connection without a password.
pub const LIBVIRT_GROUP: &str = "libvirt";

//One thing to do, with administrator rights.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub struct Step {
    //What it is for, in words: "install QEMU, libvirt and the UEFI firmware".
    pub why: String,
    //Linux: the program and its arguments. Windows: one PowerShell statement.
    pub command: Vec<String>,
}

#[cfg(any(target_os = "linux", windows, test))]
impl Step {
    fn new(why: impl Into<String>, command: &[&str]) -> Self {
        Step {
            why: why.into(),
            command: command.iter().map(|word| word.to_string()).collect(),
        }
    }
}

//How setting up ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub enum Outcome {
    //Nothing was missing.
    NothingToDo,
    //Set up, and this process can use it straight away. Windows always needs
    //a restart or a new sign-in first, so only Linux gets this.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Ready,
    //Linux: set up, but this process started before the user joined this
    //group, so it can't reach libvirt yet.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NewGroup(&'static str),
    //Windows: set up, but it takes effect only after a restart or a new
    //sign-in. Says which.
    #[cfg_attr(not(windows), allow(dead_code))]
    Restart(String),
}

//What this computer still needs, in the order to do it. Empty when it is ready.
//An error when it can't be fixed by running anything.
#[cfg(any(target_os = "linux", windows))]
pub fn plan() -> anyhow::Result<Vec<Step>> {
    platform::steps()
}

//Lists the steps, asks, and runs them. `problem` is what went wrong, shown
//first.
#[cfg(any(target_os = "linux", windows))]
pub fn set_up(ui: Ui, problem: Option<&str>, steps: &[Step]) -> AppResult<Outcome> {
    if steps.is_empty() {
        return Ok(Outcome::NothingToDo);
    }
    let app = brand::NAME;

    if let Some(problem) = problem {
        ui.warn(problem.lines().next().unwrap_or(problem));
    }
    if !ui.quiet {
        eprintln!(
            "This computer isn't set up to run VMs yet. {} can do it for you:",
            brand::DISPLAY_NAME
        );
        for step in steps {
            eprintln!("  - {}", step.why);
        }
        if let Some(note) = platform::elevation_note() {
            eprintln!("{note}");
        }
    }

    if !ui.assume_yes {
        if !ui.interactive() {
            return Err(AppError::Usage(format!(
                "this computer isn't set up to run VMs yet. Run `{app} doctor --fix` to set it up, \
                 or add --yes to let {app} do it now."
            )));
        }
        if !Confirm::new("Set it up now?").with_default(true).prompt()? {
            return Err(AppError::Cancelled);
        }
    }

    platform::run(ui, steps)?;
    Ok(platform::finish(ui, steps))
}

//For `create` and `doctor --fix`, which carry on with the hypervisor
//afterwards: false when there was nothing to do, true when it is set up and
//usable now. A Linux group this process doesn't have yet is picked up by running
//the same command again; a Windows restart can't be, so that ends the command.
#[cfg(any(target_os = "linux", windows))]
pub fn offer(ui: Ui, problem: Option<&str>) -> AppResult<bool> {
    match set_up(ui, problem, &plan()?)? {
        Outcome::NothingToDo => Ok(false),
        Outcome::Ready => Ok(true),
        Outcome::NewGroup(group) => {
            if !ui.quiet {
                eprintln!(
                    "Running it again with the {group} group, so there is no need to log out."
                );
            }
            rerun_with_new_group();
            Err(AppError::Usage(format!(
                "you were added to the {group} group, which takes effect when you log in again. \
                 Log out and back in, then run this again."
            )))
        }
        Outcome::Restart(message) => Err(AppError::Usage(message)),
    }
}

#[cfg(target_os = "linux")]
//The packages that provide QEMU, libvirt (with virsh) and the UEFI firmware, for
//one package manager and this computer's architecture. apt also installs what
//they recommend, which brings libvirt's `default` network.
fn packages(manager: &str, arm: bool) -> Vec<&'static str> {
    match (manager, arm) {
        ("apt-get", false) => vec![
            "qemu-system-x86",
            "qemu-utils",
            "libvirt-daemon-system",
            "libvirt-clients",
            "ovmf",
        ],
        ("apt-get", true) => vec![
            "qemu-system-arm",
            "qemu-utils",
            "libvirt-daemon-system",
            "libvirt-clients",
            "qemu-efi-aarch64",
        ],
        (_, false) => vec!["qemu-kvm", "qemu-img", "libvirt", "edk2-ovmf"],
        (_, true) => vec!["qemu-kvm", "qemu-img", "libvirt", "edk2-aarch64"],
    }
}

#[cfg(target_os = "linux")]
//The package with the `ssh` command.
fn ssh_package(manager: &str) -> &'static str {
    if manager == "apt-get" {
        "openssh-client"
    } else {
        "openssh-clients"
    }
}

#[cfg(target_os = "linux")]
//A command as a person would type it, for showing before it runs.
fn display(command: &[String]) -> String {
    command
        .iter()
        .map(|word| {
            if word.contains([' ', '\'', '"', ';', '|', '&']) {
                crate::seed::shell_quote(word)
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::Path;
    use std::process::{Command, Stdio};

    use anyhow::{Context, Result, bail};

    use super::{FIRMWARE_DIRS, LIBVIRT_GROUP, Outcome, Step, display, packages, ssh_package};
    use crate::brand;
    use crate::utilities::{paths, ui::Ui};

    //Fedora and RHEL run libvirt as one daemon per driver, started by sockets;
    //Debian and Ubuntu run the single libvirtd. Whichever this system has
    //enabled is the one to start.
    const START_SERVICE: &str = "if systemctl is-enabled --quiet virtqemud.socket 2>/dev/null; \
         then systemctl start virtqemud.socket virtnetworkd.socket virtstoraged.socket; \
         else systemctl enable --now libvirtd.service; fi";

    pub fn steps() -> Result<Vec<Step>> {
        if !Path::new("/dev/kvm").exists() {
            bail!(
                "/dev/kvm is missing, so this computer can't run VMs at full speed. Turn on virtualization \
                 (VT-x or AMD-V) in the firmware settings; inside a VM, turn on nested virtualization. \
                 No package can fix this."
            );
        }

        let mut steps = Vec::new();
        let missing_hypervisor = paths::find_program("virsh").is_none()
            || !qemu_installed()
            || !FIRMWARE_DIRS.iter().any(|dir| Path::new(dir).exists());
        //status, ssh, exec and logs all need the client; minimal installs and
        //containers can lack it.
        let missing_ssh = paths::find_program("ssh").is_none();
        if missing_hypervisor || missing_ssh {
            let Some(manager) = ["apt-get", "dnf"]
                .into_iter()
                .find(|manager| paths::find_program(manager).is_some())
            else {
                bail!(
                    "QEMU, libvirt, the UEFI firmware or the OpenSSH client is missing, and this Linux has \
                     neither apt nor dnf, so {} can't install them. Install them with your package manager, \
                     then run this again.",
                    brand::DISPLAY_NAME
                );
            };
            if manager == "apt-get" {
                steps.push(Step::new(
                    "refresh the package lists",
                    &["apt-get", "update"],
                ));
            }
            let mut install = vec![manager, "install", "-y"];
            if missing_hypervisor {
                install.extend(packages(manager, cfg!(target_arch = "aarch64")));
            }
            if missing_ssh {
                install.push(ssh_package(manager));
            }
            let why = match (missing_hypervisor, missing_ssh) {
                (true, true) => "install QEMU, libvirt, the UEFI firmware and the OpenSSH client",
                (true, false) => "install QEMU, libvirt and the UEFI firmware",
                (false, _) => "install the OpenSSH client",
            };
            steps.push(Step::new(why, &install));
        }

        if (missing_hypervisor || !service_running()) && paths::find_program("systemctl").is_some()
        {
            steps.push(Step::new(
                "start the libvirt service, and start it at boot",
                &["sh", "-c", START_SERVICE],
            ));
        }

        //Only the libvirt group. The system connection's own QEMU is the one
        //that opens /dev/kvm, so the user never needs the kvm group.
        let user = current_user()?;
        if !groups_of(Some(&user)).iter().any(|g| g == LIBVIRT_GROUP) {
            steps.push(Step::new(
                format!("add {user} to the {LIBVIRT_GROUP} group"),
                &["usermod", "-aG", LIBVIRT_GROUP, &user],
            ));
        }
        Ok(steps)
    }

    pub fn elevation_note() -> Option<&'static str> {
        (!is_root()).then_some("These run with sudo, so you may be asked for your password.")
    }

    pub fn run(ui: Ui, steps: &[Step]) -> Result<()> {
        let root = is_root();
        if !root && paths::find_program("sudo").is_none() {
            bail!(
                "setting up needs root, and sudo is not installed. Run `{} doctor --fix` as root.",
                brand::NAME
            );
        }
        for step in steps {
            let mut command = Vec::new();
            if !root {
                command.push("sudo".to_string());
            }
            command.extend(step.command.iter().cloned());
            if !ui.quiet {
                eprintln!("$ {}", display(&command));
            }
            //The package manager's output goes to stderr, so a --json result on
            //stdout stays clean.
            let status = Command::new(&command[0])
                .args(&command[1..])
                .stdout(to_stderr()?)
                .status()
                .with_context(|| format!("could not run {}", command[0]))?;
            if !status.success() {
                bail!(
                    "`{}` failed ({status}). Fix what it reported, then run `{} doctor --fix` again.",
                    display(&command),
                    brand::NAME
                );
            }
        }
        Ok(())
    }

    pub fn finish(ui: Ui, _steps: &[Step]) -> Outcome {
        ui.done("this computer is set up to run VMs");
        //A group just added reaches only programs started after the next login.
        if groups_of(None).iter().any(|g| g == LIBVIRT_GROUP) {
            Outcome::Ready
        } else {
            Outcome::NewGroup(LIBVIRT_GROUP)
        }
    }

    //`usermod -aG` only reaches programs started after the next login. Until
    //then this starts the same command again through `sg`, which runs it with a
    //group the user was just given, so setting up needs no logout. Called at
    //startup and after setup; it returns only when there is nothing to do or
    //`sg` could not start.
    pub fn rerun_with_new_group() {
        let guard = brand::env_var("REGROUPED");
        if std::env::var_os(&guard).is_some() || groups_of(None).iter().any(|g| g == LIBVIRT_GROUP)
        {
            return;
        }
        let Ok(user) = current_user() else { return };
        if !groups_of(Some(&user)).iter().any(|g| g == LIBVIRT_GROUP) {
            return;
        }
        let (Some(sg), Ok(exe)) = (paths::find_program("sg"), std::env::current_exe()) else {
            return;
        };
        let mut words = vec![crate::seed::shell_quote(&exe.to_string_lossy())];
        for arg in std::env::args_os().skip(1) {
            let Some(arg) = arg.to_str() else { return };
            words.push(crate::seed::shell_quote(arg));
        }
        use std::os::unix::process::CommandExt;
        //exec replaces this process, and only returns if it failed.
        let _ = Command::new(sg)
            .arg(LIBVIRT_GROUP)
            .arg("-c")
            .arg(words.join(" "))
            .env(guard, "1")
            .exec();
    }

    fn qemu_installed() -> bool {
        ["qemu-system-x86_64", "qemu-system-aarch64"]
            .iter()
            .any(|name| paths::find_program(name).is_some())
            //RHEL keeps its QEMU off the PATH.
            || Path::new("/usr/libexec/qemu-kvm").exists()
    }

    fn service_running() -> bool {
        [
            "libvirtd.service",
            "libvirtd.socket",
            "virtqemud.socket",
            "virtqemud.service",
        ]
        .iter()
        .any(|unit| {
            Command::new("systemctl")
                .args(["is-active", "--quiet", unit])
                .status()
                .is_ok_and(|status| status.success())
        })
    }

    //The groups of `user` in the system's database, or of this process when
    //None. They differ right after `usermod`, until the next login.
    fn groups_of(user: Option<&str>) -> Vec<String> {
        let mut command = Command::new("id");
        command.arg("-nG");
        if let Some(user) = user {
            command.arg(user);
        }
        command
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn current_user() -> Result<String> {
        let output = Command::new("id")
            .arg("-un")
            .output()
            .context("could not run id")?;
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if name.is_empty() {
            bail!("could not tell which user this is");
        }
        Ok(name)
    }

    fn is_root() -> bool {
        Command::new("id")
            .arg("-u")
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "0")
    }

    fn to_stderr() -> Result<Stdio> {
        use std::os::fd::AsFd;
        Ok(Stdio::from(std::io::stderr().as_fd().try_clone_to_owned()?))
    }
}

//The Windows plan, worked out from what PowerShell and whoami report. Kept
//apart from running anything so the tests can check it on every OS.
#[cfg(any(windows, test))]
mod windows_plan {
    use anyhow::{Result, bail};

    use super::Step;
    use crate::brand;
    use crate::hypervisor::hyperv::quote;

    //Hyper-V Administrators, named by SID because the group's name is
    //translated on non-English Windows.
    pub const HYPERV_ADMINS: &str = "S-1-5-32-578";
    //In `whoami /groups` when the shell is running as administrator.
    pub const ELEVATED: &str = "S-1-16-12288";

    //Hyper-V itself, its PowerShell module (every Hyper-V backend call uses
    //it), and vmconnect for the console. The same names on Windows 10, 11 and
    //Server.
    pub const FEATURES: [&str; 3] = [
        "Microsoft-Hyper-V",
        "Microsoft-Hyper-V-Management-PowerShell",
        "Microsoft-Hyper-V-Management-Clients",
    ];

    //Prints "<feature>=<InstallState>" for each Hyper-V feature (1 on, 2 off,
    //3 not available) and whether the Hyper-V service exists, which it does
    //only after the restart that finishes turning Hyper-V on. Needs no
    //administrator rights.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub const STATE_SCRIPT: &str = "Get-CimInstance -ClassName Win32_OptionalFeature -Filter \"Name LIKE 'Microsoft-Hyper-V%'\" \
         | ForEach-Object { \"$($_.Name)=$($_.InstallState)\" }\n\
         \"vmms=$([bool](Get-Service -Name vmms -ErrorAction SilentlyContinue))\"";

    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct State {
        //Features from FEATURES that are present and off.
        pub off: Vec<&'static str>,
        //Hyper-V itself is present (missing on Windows Home).
        pub available: bool,
        //Hyper-V is on but Windows hasn't restarted since.
        pub needs_restart: bool,
    }

    pub fn parse_state(text: &str) -> State {
        let mut state = State::default();
        let mut hyper_v_on = false;
        let mut service = false;
        for line in text.lines() {
            let Some((name, value)) = line.trim().split_once('=') else {
                continue;
            };
            if name == "vmms" {
                service = value.eq_ignore_ascii_case("true");
                continue;
            }
            let Some(feature) = FEATURES
                .iter()
                .find(|feature| feature.eq_ignore_ascii_case(name))
            else {
                continue;
            };
            if *feature == FEATURES[0] {
                state.available = value == "1" || value == "2";
                hyper_v_on = value == "1";
            }
            if value == "2" {
                state.off.push(feature);
            }
        }
        state.needs_restart = hyper_v_on && !service;
        state
    }

    //`groups` is what `whoami /groups` printed, `user` what `whoami` printed.
    pub fn steps(state: &State, groups: &str, user: &str) -> Result<Vec<Step>> {
        if !state.available {
            bail!(
                "this Windows has no Hyper-V. Windows Home doesn't include it: {} needs Windows 10 or 11 \
                 Pro, Enterprise or Education, or Windows Server.",
                brand::DISPLAY_NAME
            );
        }
        let mut steps = Vec::new();
        if !state.off.is_empty() {
            let features = state.off.join(", ");
            steps.push(Step::new(
                "turn on Hyper-V and its management tools (Windows restarts afterwards)",
                &[&format!(
                    "Enable-WindowsOptionalFeature -Online -FeatureName {features} -All -NoRestart"
                )],
            ));
        }
        if !groups.contains(HYPERV_ADMINS) && !groups.contains(ELEVATED) {
            steps.push(Step::new(
                format!("add {user} to the Hyper-V Administrators group"),
                &[&format!(
                    "Add-LocalGroupMember -SID {HYPERV_ADMINS} -Member {}",
                    quote(user)
                )],
            ));
        }
        if steps.is_empty() && state.needs_restart {
            bail!(
                "Hyper-V is turned on, but Windows hasn't restarted since. Restart Windows, then run this again."
            );
        }
        Ok(steps)
    }

    //Runs every step, stopping at the first error, which it writes to `log`
    //for the window that started it. A member already in the group (added
    //before the last sign-in) is not an error.
    pub fn elevated_script(steps: &[Step], log: &str) -> String {
        let mut script = String::from(
            "$ErrorActionPreference = 'Stop'\n$ProgressPreference = 'SilentlyContinue'\ntry {\n",
        );
        for statement in steps.iter().flat_map(|step| &step.command) {
            script.push_str(&format!(
                "  try {{ {statement} | Out-Null }} catch [Microsoft.PowerShell.Commands.MemberExistsException] {{ }}\n"
            ));
        }
        script.push_str(&format!(
            "}} catch {{\n  $_ | Out-String | Set-Content -LiteralPath {} -Encoding UTF8\n  exit 1\n}}\nexit 0\n",
            quote(log)
        ));
        script
    }

    //Starts `encoded` (an -EncodedCommand) in a new PowerShell with
    //administrator rights, which is what makes Windows ask, and exits with its
    //exit code.
    pub fn launcher(encoded: &str) -> String {
        format!(
            "$p = Start-Process -FilePath powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
             -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{encoded}'; exit $p.ExitCode"
        )
    }

    pub fn restart_message(steps: &[Step]) -> String {
        let turned_on = steps
            .iter()
            .flat_map(|step| &step.command)
            .any(|statement| statement.starts_with("Enable-WindowsOptionalFeature"));
        if turned_on {
            format!(
                "Restart Windows to finish turning on Hyper-V, then run `{} doctor` to check.",
                brand::NAME
            )
        } else {
            format!(
                "Sign out of Windows and back in so the Hyper-V Administrators group takes effect, then run `{} doctor` to check.",
                brand::NAME
            )
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::process::{Command, Stdio};

    use anyhow::{Context, Result, bail};

    use super::windows_plan::{
        ELEVATED, STATE_SCRIPT, elevated_script, launcher, parse_state, restart_message,
    };
    use super::{Outcome, Step};
    use crate::brand;
    use crate::hypervisor::hyperv::encode_command;
    use crate::utilities::ui::Ui;

    pub fn steps() -> Result<Vec<Step>> {
        let state = parse_state(&powershell_output(&encode_command(STATE_SCRIPT))?);
        let groups = whoami(&["/groups"])?;
        let user = whoami(&[])?;
        super::windows_plan::steps(&state, &groups, user.trim())
    }

    pub fn elevation_note() -> Option<&'static str> {
        (!elevated()).then_some("Windows will ask once for administrator rights.")
    }

    pub fn run(ui: Ui, steps: &[Step]) -> Result<()> {
        if !ui.quiet {
            for statement in steps.iter().flat_map(|step| &step.command) {
                eprintln!("PS> {statement}");
            }
        }
        let log =
            std::env::temp_dir().join(format!("{}-setup-{}.log", brand::NAME, std::process::id()));
        let _ = std::fs::remove_file(&log);
        let encoded = encode_command(&elevated_script(steps, &log.to_string_lossy()));
        //Already elevated: run it here. Otherwise through Start-Process -Verb
        //RunAs, which shows Windows' administrator prompt.
        let script = if elevated() {
            encoded
        } else {
            encode_command(&launcher(&encoded))
        };
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &script])
            .stdin(Stdio::null())
            .output()
            .context("could not run powershell.exe")?;
        let reported = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_file(&log);
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if reported.trim().is_empty() && stderr.contains("canceled by the user") {
            bail!(
                "Windows didn't get administrator rights, so nothing was changed. Run `{} doctor --fix` again \
                 and choose Yes when Windows asks.",
                brand::NAME
            );
        }
        let detail = if reported.trim().is_empty() {
            stderr.trim()
        } else {
            reported.trim()
        };
        bail!(
            "setting up failed: {detail}\nFix what it reported, then run `{} doctor --fix` again.",
            brand::NAME
        )
    }

    pub fn finish(ui: Ui, steps: &[Step]) -> Outcome {
        ui.done("Hyper-V is set up");
        Outcome::Restart(restart_message(steps))
    }

    pub fn rerun_with_new_group() {}

    fn powershell_output(encoded: &str) -> Result<String> {
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded])
            .stdin(Stdio::null())
            .output()
            .context("could not run powershell.exe")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn whoami(args: &[&str]) -> Result<String> {
        let output = Command::new("whoami.exe")
            .args(args)
            .output()
            .context("could not run whoami.exe")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn elevated() -> bool {
        whoami(&["/groups"]).is_ok_and(|groups| groups.contains(ELEVATED))
    }
}

#[cfg(any(target_os = "linux", windows))]
pub use platform::rerun_with_new_group;

//macOS has nothing to set up from here: Homebrew installs vfkit with Vmseed,
//and `doctor` explains anything else.
#[cfg(not(any(target_os = "linux", windows)))]
pub fn plan() -> anyhow::Result<Vec<Step>> {
    Ok(Vec::new())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn set_up(_ui: Ui, _problem: Option<&str>, _steps: &[Step]) -> AppResult<Outcome> {
    Ok(Outcome::NothingToDo)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn offer(_ui: Ui, _problem: Option<&str>) -> AppResult<bool> {
    let _ = AppError::Cancelled;
    Ok(false)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn rerun_with_new_group() {}

#[cfg(test)]
mod tests {
    use super::windows_plan::*;
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn each_manager_gets_qemu_libvirt_and_firmware() {
        for (manager, arm, firmware) in [
            ("apt-get", false, "ovmf"),
            ("apt-get", true, "qemu-efi-aarch64"),
            ("dnf", false, "edk2-ovmf"),
            ("dnf", true, "edk2-aarch64"),
        ] {
            let list = packages(manager, arm);
            assert!(list.contains(&firmware), "{manager} {arm}");
            assert!(
                list.iter().any(|p| p.contains("libvirt")),
                "{manager} {arm}"
            );
            assert!(
                list.iter().any(|p| p.starts_with("qemu")),
                "{manager} {arm}"
            );
        }
        assert_eq!(ssh_package("apt-get"), "openssh-client");
        assert_eq!(ssh_package("dnf"), "openssh-clients");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn commands_are_shown_the_way_they_would_be_typed() {
        let step = Step::new("x", &["sh", "-c", "a; b"]);
        let mut command = vec!["sudo".to_string()];
        command.extend(step.command);
        assert_eq!(display(&command), "sudo sh -c 'a; b'");
    }

    const USER: &str = "desk\\o'neil";

    #[test]
    fn windows_turns_on_only_the_features_that_are_off() {
        let state = parse_state(
            "Microsoft-Hyper-V-All=2\r\nMicrosoft-Hyper-V=2\r\nMicrosoft-Hyper-V-Management-PowerShell=1\r\n\
             Microsoft-Hyper-V-Management-Clients=2\r\nvmms=False\r\n",
        );
        assert!(state.available);
        assert!(!state.needs_restart);
        let steps = steps(&state, HYPERV_ADMINS, USER).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].command,
            [
                "Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V, \
              Microsoft-Hyper-V-Management-Clients -All -NoRestart"
            ]
        );
        assert!(restart_message(&steps).starts_with("Restart Windows"));
    }

    #[test]
    fn windows_adds_the_user_to_hyper_v_administrators_by_sid() {
        let state = parse_state("Microsoft-Hyper-V=1\nvmms=True\n");
        let steps = steps(&state, "BUILTIN\\Users S-1-5-32-545", USER).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].command,
            ["Add-LocalGroupMember -SID S-1-5-32-578 -Member 'desk\\o''neil'"]
        );
        assert!(restart_message(&steps).starts_with("Sign out"));
        //An administrator shell can already manage Hyper-V.
        assert!(steps_for_elevated(&state).is_empty());
    }

    fn steps_for_elevated(state: &State) -> Vec<Step> {
        steps(state, ELEVATED, USER).unwrap()
    }

    #[test]
    fn windows_without_hyper_v_or_waiting_for_a_restart_is_explained() {
        //Windows Home lists no Hyper-V feature at all.
        let home = parse_state("vmms=False\n");
        assert!(!home.available);
        assert!(
            steps(&home, ELEVATED, USER)
                .unwrap_err()
                .to_string()
                .contains("Windows Home")
        );
        let pending = parse_state(
            "Microsoft-Hyper-V=1\nMicrosoft-Hyper-V-Management-PowerShell=1\nvmms=False\n",
        );
        assert!(pending.needs_restart);
        assert!(
            steps(&pending, ELEVATED, USER)
                .unwrap_err()
                .to_string()
                .contains("hasn't restarted")
        );
        assert!(steps_for_elevated(&parse_state("Microsoft-Hyper-V=1\nvmms=True\n")).is_empty());
    }

    #[test]
    fn the_elevated_script_runs_every_step_and_reports_errors() {
        let steps = vec![
            Step::new(
                "a",
                &["Enable-WindowsOptionalFeature -Online -FeatureName X -All -NoRestart"],
            ),
            Step::new("b", &["Add-LocalGroupMember -SID S-1-5-32-578 -Member 'u'"]),
        ];
        let script = elevated_script(&steps, "C:\\Temp\\it's.log");
        assert!(script.starts_with("$ErrorActionPreference = 'Stop'"));
        assert!(script.contains("try { Enable-WindowsOptionalFeature"));
        assert!(script.contains("try { Add-LocalGroupMember"));
        assert!(script.contains("MemberExistsException"));
        assert!(script.contains("-LiteralPath 'C:\\Temp\\it''s.log'"));
        assert!(launcher("QUJD").contains("-Verb RunAs"));
        assert!(launcher("QUJD").contains("'-EncodedCommand','QUJD'"));
    }

    //Runs the real query on a Windows computer (CI's Windows runner), so a
    //PowerShell mistake in it fails here rather than for a user.
    #[cfg(windows)]
    #[test]
    fn windows_state_can_be_read() {
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &crate::hypervisor::hyperv::encode_command(STATE_SCRIPT),
            ])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{text}");
        assert!(text.contains("vmms="), "{text}");
    }
}
