//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Runs the real program the way a person would, in throwaway folders. Each test
//points VMSEED_HOME at its own folder, so nothing touches the real ~/.vmseed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

//The binary cargo built for these tests, and its name ("vmseed"), which follows
//the package name so a rename does not break the tests.
const BINARY: &str = env!("CARGO_BIN_EXE_vmseed");
const NAME: &str = env!("CARGO_PKG_NAME");

//A fresh folder for one test, with a project folder and a home folder inside.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!("{NAME}-cli-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("project")).unwrap();
        Sandbox { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(BINARY)
            .args(args)
            .current_dir(cwd)
            .env(
                format!("{}_HOME", NAME.to_uppercase()),
                self.root.join("home"),
            )
            .env("NO_COLOR", "1")
            .output()
            .expect("the binary runs")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn bare_command_prints_grouped_help_and_succeeds() {
    let sandbox = Sandbox::new("help");
    let output = sandbox.run(&sandbox.project(), &[]);
    assert!(output.status.success());
    let text = stdout(&output);
    for heading in [
        "Project:",
        "Lifecycle:",
        "Inspect:",
        "Tools:",
        "Global options:",
    ] {
        assert!(text.contains(heading), "missing {heading} in:\n{text}");
    }
}

#[test]
fn every_version_flag_prints_the_version() {
    let sandbox = Sandbox::new("version");
    for flag in ["-v", "-V", "--version", "version"] {
        let output = sandbox.run(&sandbox.project(), &[flag]);
        assert!(output.status.success(), "{flag}");
        assert!(
            stdout(&output).starts_with(&format!("{NAME} {}", env!("CARGO_PKG_VERSION"))),
            "{flag}"
        );
    }
}

#[test]
fn typos_are_usage_errors() {
    let sandbox = Sandbox::new("typo");
    let output = sandbox.run(&sandbox.project(), &["strat"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn vm_commands_outside_a_project_explain_themselves() {
    let sandbox = Sandbox::new("noproject");
    let output = sandbox.run(&sandbox.root, &["start"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("not in a"), "{}", stderr(&output));
}

//The whole life of a VM, on the pretend hypervisor debug builds have.
#[test]
fn a_vm_is_created_used_and_removed() {
    let sandbox = Sandbox::new("lifecycle");
    let project = sandbox.project();
    let run = |args: &[&str]| {
        Command::new(BINARY)
            .args(args)
            .current_dir(&project)
            .env(
                format!("{}_HOME", NAME.to_uppercase()),
                sandbox.root.join("home"),
            )
            .env(format!("{}_HYPERVISOR", NAME.to_uppercase()), "fake")
            .env("NO_COLOR", "1")
            .output()
            .expect("the binary runs")
    };
    let ok = |args: &[&str]| {
        let output = run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}{}",
            stdout(&output),
            stderr(&output)
        );
        output
    };

    ok(&[
        "init",
        "--name",
        "shop",
        "--blueprint",
        "ubuntu-server",
        "--username",
        "dev",
        "--password-hash",
        "$6$salt$hash",
        "-y",
    ]);
    std::fs::write(project.join("fake.iso"), b"iso").unwrap();
    let local = project
        .join(format!(".{NAME}"))
        .join("vm.config.local.yaml");
    let mut text = std::fs::read_to_string(&local).unwrap();
    text.push_str(&format!(
        "isoPath: '{}'\n",
        project.join("fake.iso").display()
    ));
    std::fs::write(&local, text).unwrap();

    //--render writes the seed for review and creates nothing.
    ok(&["create", "--render"]);
    let created = ok(&["create", "--json"]);
    let result: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(result["name"], "shop-ubuntu-server");
    assert_eq!(result["state"], "running");

    //A second create refuses rather than overwriting.
    assert_eq!(run(&["create"]).status.code(), Some(2));

    let status: serde_json::Value =
        serde_json::from_slice(&ok(&["status", "--json", "--no-ssh"]).stdout).unwrap();
    assert_eq!(status["state"], "running");
    assert_eq!(status["bootstrap"], "unknown");

    let list: serde_json::Value = serde_json::from_slice(&ok(&["list", "--json"]).stdout).unwrap();
    assert_eq!(list[0]["status"], "running");

    ok(&["stop"]);
    let snapshots: serde_json::Value =
        serde_json::from_slice(&ok(&["snapshot", "list", "--json"]).stdout).unwrap();
    assert_eq!(
        snapshots[0]["name"], "clean-install",
        "create takes one after the install"
    );
    ok(&["snapshot", "create", "before-upgrade"]);
    ok(&["snapshot", "restore", "before-upgrade", "--yes"]);
    ok(&["start"]);
    ok(&["restart"]);

    //Found by name from anywhere, too.
    let elsewhere = Command::new(BINARY)
        .args(["status", "shop-ubuntu-server", "--json", "--no-ssh"])
        .current_dir(&sandbox.root)
        .env(
            format!("{}_HOME", NAME.to_uppercase()),
            sandbox.root.join("home"),
        )
        .env(format!("{}_HYPERVISOR", NAME.to_uppercase()), "fake")
        .output()
        .unwrap();
    assert!(elsewhere.status.success());

    //Removing asks first, and refuses to guess without a terminal.
    assert_eq!(run(&["remove"]).status.code(), Some(2));
    ok(&["remove", "--yes", "--delete-disks"]);
    let list: serde_json::Value = serde_json::from_slice(&ok(&["list", "--json"]).stdout).unwrap();
    assert_eq!(list[0]["status"], "not created");
}

#[test]
fn init_writes_the_project_and_list_finds_it() {
    let sandbox = Sandbox::new("init");
    let project = sandbox.project();

    let hash = sandbox.run(&project, &["hash", "--stdin"]);
    //No stdin was given, so the password is empty and refused.
    assert_eq!(hash.status.code(), Some(2));

    let output = sandbox.run(
        &project,
        &[
            "init",
            "--name",
            "acme",
            "--username",
            "dev",
            "--password-hash",
            "$6$salt$hash",
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let folder = project.join(format!(".{NAME}"));
    for file in [
        "vm.config.yaml",
        "vm.config.local.yaml",
        "vm.config.example.yaml",
        "vm.config.local.example.yaml",
        ".gitignore",
        "ssh_keys/README.md",
    ] {
        assert!(folder.join(file).is_file(), "{file} was not written");
    }
    let shared = std::fs::read_to_string(folder.join("vm.config.yaml")).unwrap();
    assert!(shared.contains("projectName:    acme"));
    let local = std::fs::read_to_string(folder.join("vm.config.local.yaml")).unwrap();
    assert!(local.contains("passwordHash:   '$6$salt$hash'"));

    //A second init refuses; --force only refreshes the examples.
    assert_eq!(
        sandbox.run(&project, &["init", "--yes"]).status.code(),
        Some(2)
    );
    assert!(sandbox.run(&project, &["init", "--force"]).status.success());
    assert_eq!(
        std::fs::read_to_string(folder.join("vm.config.local.yaml")).unwrap(),
        local
    );

    //Commands work from a subfolder, the way git finds .git.
    let nested = project.join("src").join("deep");
    std::fs::create_dir_all(&nested).unwrap();
    assert!(
        sandbox
            .run(&nested, &["config", "--validate"])
            .status
            .success()
    );

    let list = sandbox.run(&sandbox.root, &["list", "--json"]);
    assert!(list.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(rows[0]["project"], "acme");
    assert_eq!(rows[0]["url"], "http://acme-redhat-rocky-workstation.local");
}

#[test]
fn init_without_a_user_needs_no_user_flag_when_unattended() {
    let sandbox = Sandbox::new("nouser");
    let project = sandbox.project();
    let refused = sandbox.run(&project, &["init", "--yes"]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(stderr(&refused).contains("--no-user"));
    assert!(
        sandbox
            .run(&project, &["init", "--no-user", "--yes"])
            .status
            .success()
    );
}

#[test]
fn custom_blueprints_are_copied_into_the_project() {
    let sandbox = Sandbox::new("custom");
    let project = sandbox.project();
    let output = sandbox.run(
        &project,
        &[
            "init",
            "--name",
            "shop",
            "--blueprint",
            "my-rocky",
            "--from",
            "redhat-rocky-workstation",
            "--no-user",
            "-y",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let manifest = project
        .join(format!(".{NAME}"))
        .join("blueprints/my-rocky/blueprint.yaml");
    assert!(
        std::fs::read_to_string(manifest)
            .unwrap()
            .contains("name: 'my-rocky'")
    );

    let list = sandbox.run(&project, &["blueprints", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    let mine = items
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "my-rocky")
        .unwrap();
    assert_eq!(mine["source"], "project");
}

#[test]
fn unknown_settings_are_reported() {
    let sandbox = Sandbox::new("badkey");
    let project = sandbox.project();
    assert!(
        sandbox
            .run(&project, &["init", "--no-user", "-y"])
            .status
            .success()
    );
    let local = project
        .join(format!(".{NAME}"))
        .join("vm.config.local.yaml");
    std::fs::write(&local, "cpuss: 4\n").unwrap();
    let output = sandbox.run(&project, &["config"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("Did you mean 'cpus'"));
}
