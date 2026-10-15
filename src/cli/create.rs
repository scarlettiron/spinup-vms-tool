//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt create`: builds a VM from the project's config. This is
//scripts/New-DevVM.ps1. The steps:
//
//  1. plan        load and check the config, show what will be built
//  2. name        refuse a VM name or hostname that is already taken
//  3. media       download and verify the install ISO (cached)
//  4. seed        render the unattended install and write the seed disk
//  5. define      create the VM with the ISO and seed attached
//  6. install     boot it and wait for the installer to power it off
//  7. finish      eject the ISO, drop the seed, attach the data disks
//  8. checkpoint  (optional) snapshot the clean install, then boot
//
//Bootstrap then runs inside the guest on its first boot; `status` follows it.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use serde_norway::Mapping;

use super::args::CreateArgs;
use super::common::{again, minutes};
use crate::brand;
use crate::config::effective::Effective;
use crate::errors::{AppError, AppResult};
use crate::guest::{self, keys};
use crate::hypervisor::{self, DataDiskSpec, Hypervisor, VmSpec, VmState};
use crate::media;
use crate::plans;
use crate::platform::HypervisorKind;
use crate::registry::Registry;
use crate::seed::{self, disk};
use crate::utilities::{paths, ui::Ui};
use crate::vms::{self, Record};

const CHECKPOINT_LABEL: &str = "clean-install";

pub fn run(args: CreateArgs, ui: Ui) -> AppResult<()> {
    let targets = if args.target.all {
        match paths::current_project_root()? {
            Some(root) => vms::project_targets(&root)?,
            None => {
                return Err(AppError::Usage(format!(
                    "`create --all` builds every machine of the project in this folder; run it inside a {} project",
                    brand::NAME
                )));
            }
        }
    } else {
        vms::resolve(args.target.name.as_deref(), false)?
    };
    let mut builds: Vec<(PathBuf, Option<String>)> = Vec::new();
    for target in &targets {
        match target.project_and_machine() {
            Some(found) => builds.push(found),
            None => {
                return Err(AppError::NotFound(format!(
                    "'{}' is not a VM or machine in any {} project. Run `{} create` inside the project folder.",
                    target.vm_name,
                    brand::NAME,
                    brand::NAME
                )));
            }
        }
    }

    //Two machines resolving to one name would overwrite each other.
    if builds.len() > 1 {
        let mut names: Vec<(String, String)> = Vec::new();
        for (root, machine) in &builds {
            let config = Effective::load(root, machine.as_deref(), Mapping::new())?;
            for (name, host) in &names {
                if *name == config.vm_name || *host == config.hostname {
                    return Err(AppError::Usage(format!(
                        "two machines resolve to the VM name or hostname '{}'. Give each machine its own vmName and hostname.",
                        config.vm_name
                    )));
                }
            }
            names.push((config.vm_name.clone(), config.hostname.clone()));
        }
        ui.heading(&format!(
            "Building {} machines: {}",
            names.len(),
            names
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let mut built: Vec<String> = Vec::new();
    let mut results = Vec::new();
    for (root, machine) in builds {
        match build(&root, machine.as_deref(), &args, ui) {
            Ok(result) => {
                built.push(result["name"].as_str().unwrap_or_default().to_string());
                results.push(result);
            }
            Err(error) => {
                if !built.is_empty() {
                    ui.warn(&format!(
                        "already built before this failure, and left in place: {}",
                        built.join(", ")
                    ));
                }
                return Err(error);
            }
        }
    }
    if ui.json {
        ui.json(&if results.len() == 1 {
            results.remove(0)
        } else {
            serde_json::Value::Array(results)
        });
    }
    Ok(())
}

//The hypervisor's view of the config.
pub fn spec_for(config: &Effective, hv: &dyn Hypervisor, iso: PathBuf) -> anyhow::Result<VmSpec> {
    let gb = |path: &str, default: u64| config.number(path, default) * 1024;
    let memory_mb = gb("VM.MemoryStartupGB", 8);
    let memory_min_mb = gb("VM.MemoryMinimumGB", 2).min(memory_mb);
    let memory_max_mb = gb("VM.MemoryMaximumGB", 16).max(memory_mb);
    let (width, height) = config
        .string_or("Bootstrap.DisplayResolution", "1920x1080")
        .split_once('x')
        .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
        .unwrap_or((1920, 1080));
    let vm_dir = config.vm_dir();
    Ok(VmSpec {
        name: config.vm_name.clone(),
        hostname: config.hostname.clone(),
        arch: config.arch,
        cpus: config.number("VM.CpuCount", 4).max(1) as u32,
        memory_mb,
        memory_min_mb,
        memory_max_mb,
        dynamic_memory: config.flag("VM.DynamicMemory", true),
        disk_gb: config.number("VM.DiskSizeGB", 40).max(10),
        data_disks: plans::data_disks(config)?
            .iter()
            .map(DataDiskSpec::from)
            .collect(),
        secure_boot: config.flag("VM.SecureBoot", true),
        secure_boot_template: config
            .blueprint
            .get_string("Firmware.SecureBootTemplate")
            .unwrap_or_else(|| "MicrosoftUEFICertificateAuthority".to_string()),
        tpm: config.blueprint.get_bool("Firmware.EnableTpm", false)
            || config.flag("VM.EnableTpm", false),
        nested: config.flag("VM.NestedVirtualization", false),
        enhanced_session: config.flag("VM.EnhancedSession", true),
        auto_checkpoints: config.flag("VM.AutomaticCheckpoints", false),
        switch_name: config.string_or("VM.SwitchName", "Default Switch"),
        network: config.string_or("VM.Network", "default"),
        storage_pool: config.string_or("VM.StoragePool", "default"),
        seed: vm_dir.join(hv.seed_file_name(&config.vm_name)),
        vm_dir,
        iso,
        project: config.project_root.clone(),
        machine: config.machine.clone(),
        display: (width, height),
    })
}

fn build(
    root: &std::path::Path,
    machine: Option<&str>,
    args: &CreateArgs,
    ui: Ui,
) -> AppResult<serde_json::Value> {
    let config = Effective::load(root, machine, Mapping::new())?;
    let hv = hypervisor::for_kind(effective_kind(&config));
    let checkpoint = !args.no_checkpoint && config.flag("VM.CheckpointAfterInstall", true);
    let total = if args.render {
        2
    } else if checkpoint {
        8
    } else {
        7
    };
    let name = config.vm_name.clone();

    ui.step(1, total, &format!("Planning {name}"));
    let warnings = plans::check_all(&config)?;
    plans::check_user(&config)?;
    show_plan(&config, hv.as_ref(), ui)?;
    for warning in &warnings {
        ui.warn(warning);
    }

    if args.render {
        ui.step(2, total, "Rendering the seed (nothing is created)");
        let own_key = keys::ensure_own_key()?;
        let target = seed::Target {
            hypervisor: hv.kind(),
            system_disk: hv.system_disk().to_string(),
        };
        let rendered = seed::render(&config, &target, own_key.as_deref())?;
        for warning in &rendered.warnings {
            ui.warn(warning);
        }
        let folder = config.vm_dir().join("build");
        let main = rendered.save_rendered(&folder)?;
        ui.info(&format!(
            "{} seed, label {}, {} file(s) embedded",
            rendered.method, rendered.label, rendered.payload_count
        ));
        ui.done_with_note(
            &format!("rendered {}", paths::display_path(&main)),
            "review it, then run create without --render",
        );
        return Ok(
            serde_json::json!({ "name": name, "rendered": main, "method": rendered.method }),
        );
    }

    ui.step(2, total, "Checking the name is free");
    hv.preflight()?;
    let existing = hv.state(&name)?;
    if existing != VmState::Missing {
        if !args.force {
            return Err(AppError::Usage(format!(
                "a VM named '{name}' already exists (state: {}). Start it with `{app} start`, or rebuild it from scratch \
                 with `{app} create --force`, which deletes it first.",
                existing.label(),
                app = brand::NAME
            )));
        }
        ui.warn(&format!(
            "removing the existing VM '{name}' because --force was given"
        ));
        hv.remove(&name, &config.vm_dir(), true)?;
        Record::delete(&name)?;
    } else if let Some(address) = guest::resolves(&format!("{}.local", config.hostname))
        && !args.force
    {
        return Err(AppError::Usage(format!(
            "the hostname '{host}' already answers on this network, at {address}. Something else uses that name: another \
             VM, a teammate's computer, or an earlier build. Change projectName (or hostname) in the config, or check \
             with: ping {host}.local",
            host = config.hostname
        )));
    }
    ui.info(&format!(
        "'{name}' is free, and {}.local answers to nothing yet",
        config.hostname
    ));

    ui.step(3, total, "Resolving install media");
    let iso = media::resolve(&config, ui)?;
    ui.info(&paths::display_path(&iso));

    ui.step(4, total, "Building the seed disk");
    let own_key = keys::ensure_own_key()?;
    if own_key.is_none() {
        ui.warn("ssh-keygen was not found, so `ssh`, `exec` and `status` will need your own key in .vmynt/ssh_keys/");
    }
    let spec = spec_for(&config, hv.as_ref(), iso)?;
    let target = seed::Target {
        hypervisor: hv.kind(),
        system_disk: hv.system_disk().to_string(),
    };
    let rendered = seed::render(&config, &target, own_key.as_deref())?;
    for warning in &rendered.warnings {
        ui.warn(warning);
    }
    for note in &rendered.notes {
        ui.info(note);
    }
    std::fs::create_dir_all(&spec.vm_dir)
        .with_context(|| format!("could not create {}", spec.vm_dir.display()))?;
    rendered.save_rendered(&spec.vm_dir.join("build"))?;
    disk::write(
        &spec.seed,
        &rendered.label,
        &rendered.disk_files,
        hv.seed_format(),
    )?;
    ui.info(&format!(
        "{} seed ({}), {} file(s) embedded",
        rendered.method, rendered.label, rendered.payload_count
    ));

    ui.step(
        5,
        total,
        &format!("Creating the {} VM '{name}'", hv.kind().display()),
    );
    ui.info(&format!(
        "{} vCPU, {} GB RAM, {} GB system disk, {} data disk(s) attached after the install",
        spec.cpus,
        spec.memory_mb / 1024,
        spec.disk_gb,
        spec.data_disks.len()
    ));
    hv.create(&spec)?;
    let mut record = Record {
        name: name.clone(),
        hostname: config.hostname.clone(),
        user: config.username(),
        project: config.project_root.clone(),
        machine: config.machine.clone(),
        hypervisor: hv.kind(),
        blueprint: config.blueprint.name.clone(),
        vm_dir: spec.vm_dir.clone(),
        created: humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
        installed: false,
        spec: Some(spec.clone()),
    };
    record.save()?;
    //A fresh install means a new host key under the old name.
    if keys::forget_host(&[&config.hostname])? {
        ui.info(&format!(
            "forgot the old host key for {}.local (this is a fresh install)",
            config.hostname
        ));
    }

    ui.step(6, total, "Running the unattended install");
    let timeout_minutes = config
        .blueprint
        .get("Provisioning.InstallTimeoutMinutes")
        .and_then(crate::config::value_as_u64)
        .unwrap_or(60);
    match config.blueprint.get_string("Provisioning.InstallSource") {
        None => ui.info("installing the base system from the ISO; the desktop and tools come later, during bootstrap"),
        Some(source) if source.trim_start().starts_with("cdrom") => ui.info("installing from the DVD, so nothing is downloaded"),
        Some(_) => ui.info("the image holds only the installer, so packages come from the mirror now; most of the wait is that download"),
    }
    ui.info(&format!(
        "usually 10-20 minutes. Watch it with: {}",
        again(
            "open --console",
            &vms::Target {
                vm_name: name.clone(),
                hostname: config.hostname.clone(),
                user: None,
                project: Some(config.project_root.clone()),
                machine: config.machine.clone(),
                record: None,
            }
        )
    ));
    //Ubuntu's installer asks "Continue with autoinstall?" unless `autoinstall`
    //is on the kernel command line, which a stock ISO can't have. The answer is
    //typed into the VM's keyboard; a backend that can't type opens the window
    //so the person can.
    let confirm = config.blueprint.get_string("Provisioning.ConfirmText");
    let can_type = confirm.is_none() || hv.kind() != crate::platform::HypervisorKind::Vfkit;
    if !can_type {
        ui.warn(&format!(
            "the installer will ask \"Continue with autoinstall?\". The install window is opening: type {} and press Enter there.",
            confirm.as_deref().unwrap_or("yes")
        ));
    }
    hv.start(&name, !can_type)?;
    let installed = wait_for_install(
        hv.as_ref(),
        &name,
        Duration::from_secs(timeout_minutes * 60),
        confirm.as_deref().filter(|_| can_type),
        ui,
    )?;
    if !installed {
        return Err(AppError::Other(anyhow::anyhow!(
            "the install did not finish within {timeout_minutes} minutes. Open the console with `{} open --console {name}` \
             to see where it stopped; an installer sitting at a prompt means it did not find the seed disk.",
            brand::NAME
        )));
    }
    ui.info("the installer finished and powered the VM off");

    ui.step(7, total, "Finishing the install");
    hv.finish_install(&spec)?;
    //The seed held the password hash and any secrets; it has done its job.
    let _ = std::fs::remove_file(&spec.seed);
    ui.info("ejected the installer, removed the seed disk, attached the data disks");

    //Taken now, with the final hardware and before the first boot runs
    //bootstrap, so rolling back to it gives a clean OS that provisions itself
    //again, and never brings back the ISO or the seed.
    if checkpoint {
        ui.step(8, total, "Checkpointing the clean install");
        match hv.snapshot_create(&name, CHECKPOINT_LABEL) {
            Ok(()) => ui.info(&format!(
                "snapshot '{CHECKPOINT_LABEL}', taken before bootstrap, so a broken tool install can be rolled back"
            )),
            Err(error) => ui.warn(&format!("could not take the clean-install snapshot: {error:#}")),
        }
    }

    ui.info("booting; bootstrap provisions the VM on this first boot");
    hv.start(&name, false)?;
    record.installed = true;
    record.save()?;
    let mut registry = Registry::load()?;
    if !registry
        .projects
        .values()
        .any(|p| p.path == config.project_root)
    {
        registry.register(
            &crate::config::get_string(&config.raw, "ProjectName").unwrap_or_else(|| name.clone()),
            &config.project_root,
            &config.blueprint.name,
        );
        registry.save()?;
    }

    let target = vms::Target {
        vm_name: name.clone(),
        hostname: config.hostname.clone(),
        user: config.username(),
        project: Some(config.project_root.clone()),
        machine: config.machine.clone(),
        record: Some(record),
    };
    ui.done(&format!("{name} is installing its desktop and tools now"));
    ui.hint("  It reboots itself once bootstrap finishes; expect 15-30 minutes on a fresh build.");
    ui.line(&format!(
        "  Follow it:         {}",
        again("status --follow", &target)
    ));
    ui.line(&format!("  Open the desktop:  {}", again("open", &target)));
    ui.line(&format!("  Shell:             {}", again("ssh", &target)));
    ui.line(&format!("  Address:           {}", config.url()));
    Ok(serde_json::json!({
        "name": name,
        "hostname": config.hostname,
        "url": config.url(),
        "hypervisor": hv.kind().as_str(),
        "state": "running",
    }))
}

//`hypervisor: fake` only exists in debug builds, for the tests.
fn effective_kind(config: &Effective) -> HypervisorKind {
    if config.hypervisor == HypervisorKind::host() {
        hypervisor::host().kind()
    } else {
        config.hypervisor
    }
}

//What will be built, the way New-DevVM.ps1's Show-Plan says it.
fn show_plan(config: &Effective, hv: &dyn Hypervisor, ui: Ui) -> AppResult<()> {
    let b = |path: &str| config.flag(path, false);
    let chain = &config.blueprint.chain;
    let inherits = if chain.len() > 1 {
        format!(", inherits {}", chain[1..].join(", "))
    } else {
        String::new()
    };
    ui.info(&format!(
        "blueprint      {}{inherits} ({}, {})",
        config.blueprint.name,
        config.os_description(),
        config.arch.as_str()
    ));
    ui.info(&format!("hypervisor     {}", hv.kind().display()));
    ui.info(&format!(
        "hardware       {} vCPU / {} GB RAM / {} GB system disk",
        config.number("VM.CpuCount", 4),
        config.number("VM.MemoryStartupGB", 8),
        config.number("VM.DiskSizeGB", 40)
    ));
    for disk in plans::data_disks(config)? {
        let drive = if disk.drive_letter.is_empty() {
            "disk".to_string()
        } else {
            format!("{}:", disk.drive_letter)
        };
        ui.info(&format!(
            "data disk      {drive:<4} {:<10} {:>4} GB at {}",
            disk.label, disk.size_gb, disk.mount_point
        ));
    }
    ui.info(&format!(
        "guest          user '{}', time zone {}",
        config.username().unwrap_or_default(),
        config.string_or("Guest.Timezone", "UTC")
    ));
    if b("Bootstrap.EnableMdns") {
        ui.info(&format!("address        {}", config.url()));
    } else {
        ui.info(&format!(
            "hostname       {} (mDNS off, so no .local address)",
            config.hostname
        ));
    }
    let extras: Vec<&str> = [
        ("desktop", "Bootstrap.InstallDesktop"),
        ("docker", "Bootstrap.InstallDocker"),
        ("ddev", "Bootstrap.InstallDdev"),
        ("node", "Bootstrap.InstallNode"),
        ("python", "Bootstrap.InstallPython"),
        ("vscode", "Bootstrap.InstallVSCode"),
        ("chrome", "Bootstrap.InstallChrome"),
        ("postman", "Bootstrap.InstallPostman"),
        ("remote-access", "Bootstrap.InstallRemoteAccess"),
    ]
    .iter()
    .filter(|(_, path)| b(path))
    .map(|(name, _)| *name)
    .collect();
    ui.info(&format!("bootstrap      {}", extras.join(", ")));
    if let Some(db) = plans::database(config)? {
        let what = if db.install_server {
            "client + server"
        } else {
            "client only"
        };
        ui.info(&format!(
            "database       {} {}, {what}",
            db.display_name, db.version
        ));
        if db.install_server {
            let reach = if db.remote {
                "reachable from other machines (firewall opened)"
            } else {
                "localhost only"
            };
            ui.info(&format!("               port {}, {reach}", db.port));
        }
        if db.adminer_install {
            ui.info(&format!(
                "adminer        http://{}.local:{}/",
                config.hostname, db.adminer_port
            ));
        }
    }
    let copies = plans::file_copies(config)?;
    if !copies.entries.is_empty() {
        let files: usize = copies.entries.iter().map(|e| e.files.len()).sum();
        let bytes: u64 = copies.entries.iter().map(|e| e.size).sum();
        ui.info(&format!(
            "files          {files} file(s), {}, from {} folder(s)",
            hypervisor::human_size(bytes),
            copies.entries.len()
        ));
        for entry in &copies.entries {
            ui.info(&format!(
                "               {} to {} (owner {})",
                paths::display_path(&entry.from),
                entry.to,
                entry.owner
            ));
        }
    }
    let repos = plans::repos(config)?;
    if !repos.is_empty() {
        ui.info(&format!(
            "repos          {} into ~/{}",
            repos.len(),
            plans::repo_root(config)?
        ));
        for repo in &repos {
            ui.info(&format!(
                "               {:<22} from {}",
                repo.directory, repo.url
            ));
        }
    }
    if let Some(hooks) = plans::hooks(config)? {
        let when = if hooks.every_boot {
            "on every boot"
        } else {
            "once, after provisioning"
        };
        ui.info(&format!(
            "hooks          {} command(s), as {}, {when}",
            hooks.commands.len(),
            hooks.run_as
        ));
    }
    Ok(())
}

//Waits for the installer to power the VM off, reporting progress every two
//minutes. When the blueprint's installer needs a typed confirmation, types it
//from 75 seconds in (well after GRUB's menu, where letters would edit the boot
//entry) until the system disk starts filling, which means it was accepted.
//Text typed before the prompt appears waits in the console's input buffer.
fn wait_for_install(
    hv: &dyn Hypervisor,
    name: &str,
    timeout: Duration,
    confirm: Option<&str>,
    ui: Ui,
) -> anyhow::Result<bool> {
    const ANSWER_FROM: Duration = Duration::from_secs(75);
    const ANSWER_EVERY: Duration = Duration::from_secs(45);
    const ANSWER_UNTIL: Duration = Duration::from_secs(15 * 60);
    const STARTED_BYTES: u64 = 256 << 20;
    let start = std::time::Instant::now();
    let mut next_tick = Duration::from_secs(120);
    let mut next_answer = ANSWER_FROM;
    let mut answered = confirm.is_none();
    let mut last_bytes = 0u64;
    while start.elapsed() < timeout {
        if hv.state(name)? == VmState::Off {
            return Ok(true);
        }
        let elapsed = start.elapsed();
        if !answered && elapsed >= next_answer {
            let written = hv.system_disk_bytes(name).unwrap_or(0);
            if written >= STARTED_BYTES || elapsed >= ANSWER_UNTIL {
                answered = true;
            } else if let Some(text) = confirm {
                match hv.type_text(name, text) {
                    Ok(()) => {
                        ui.detail(&format!("typed '{text}' for the installer's confirmation"))
                    }
                    Err(error) => {
                        ui.warn(&format!(
                            "could not answer the installer's confirmation: {error:#}"
                        ));
                        answered = true;
                    }
                }
                next_answer += ANSWER_EVERY;
            }
        }
        if elapsed >= next_tick {
            let mut line = format!(
                "still installing, {} of {} min allowed",
                minutes(elapsed),
                timeout.as_secs() / 60
            );
            if let Some(bytes) = hv.system_disk_bytes(name) {
                line.push_str(&format!(", system disk {}", hypervisor::human_size(bytes)));
                if last_bytes > 0 && bytes <= last_bytes {
                    line.push_str(" (no growth since the last check)");
                }
                last_bytes = bytes;
            }
            ui.info(&line);
            next_tick += Duration::from_secs(120);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Ok(false)
}
