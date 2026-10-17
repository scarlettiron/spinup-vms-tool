//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Turning the config into unattended-install files: the kickstart or cloud-init
//seed, with the whole bootstrap tree and its generated settings embedded, so
//the finished VM carries its own provisioning.

pub mod disk;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::config::effective::{Effective, string_list};
use crate::passwd;
use crate::plans::{self, KeySource};
use crate::platform::HypervisorKind;

//Where staged secrets land in the guest. Bootstrap step 55 installs them and
//shreds them.
const SECRETS_DIR: &str = "/root/vm-bootstrap-secrets";
const DEFAULT_BOOTSTRAP_ROOT: &str = "/usr/local/lib/vm-bootstrap";

//What the hypervisor tells the renderer.
#[derive(Debug, Clone)]
pub struct Target {
    pub hypervisor: HypervisorKind,
    //The installer's name for the system disk: sda on SCSI, vda on virtio.
    pub system_disk: String,
}

//One file the guest receives.
#[derive(Debug, Clone)]
struct Payload {
    target: String,
    permissions: String,
    contents: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Seed {
    pub method: String,
    pub label: String,
    //The rendered seed files (ks.cfg, or user-data and meta-data).
    pub seed_files: Vec<(String, String)>,
    //Everything that goes on the seed disk: just the seed files, at the root.
    //The bootstrap tree is already inside them; a plain copy for reading is
    //saved beside the VM in build/bootstrap/ instead.
    pub disk_files: Vec<(String, Vec<u8>)>,
    //What `save_rendered` writes: the seed files and that plain copy.
    pub review_files: Vec<(String, Vec<u8>)>,
    pub payload_count: usize,
    //What the build should say: secrets embedded, key source, file copies.
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

impl Seed {
    //Saves the rendered seed files for review, in a private folder.
    pub fn save_rendered(&self, dir: &Path) -> Result<PathBuf> {
        if dir.exists() {
            std::fs::remove_dir_all(dir).ok();
        }
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
        restrict_dir(dir);
        for (name, contents) in &self.review_files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, contents)?;
        }
        Ok(dir.join(&self.seed_files[0].0))
    }
}

fn restrict_dir(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = dir;
}

fn shell_bool(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn base64_lines(bytes: &[u8]) -> Vec<String> {
    let encoded = BASE64.encode(bytes);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

//Unix text: no BOM, LF endings. cloud-init ignores a user-data file with a BOM.
fn unix_text(text: &str) -> Vec<u8> {
    text.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .into_bytes()
}

//The primary user's password hash: the one given, or a fresh hash of the
//plain password.
pub fn password_hash(config: &Effective) -> Result<String> {
    if let Some(hash) = config.string("Guest.PrimaryUser.PasswordHash") {
        if !hash.starts_with('$') {
            bail!("passwordHash does not look like a crypt hash");
        }
        //Hashes from older versions carry a long salt glibc can't match.
        return Ok(passwd::to_glibc_form(&hash));
    }
    if let Some(password) = config.string("Guest.PrimaryUser.Password") {
        return passwd::hash_password(&password);
    }
    bail!("the user needs a password: set passwordHash (make one with `hash`) or password")
}

//Renders everything. `own_key` is Vmseed's public key, added to the user's
//authorized keys so `ssh`, `exec` and `status` work without setup.
pub fn render(config: &Effective, target: &Target, own_key: Option<&str>) -> Result<Seed> {
    let blueprint = &config.blueprint;
    let bootstrap_root = blueprint
        .get_string("Provisioning.BootstrapRoot")
        .unwrap_or_else(|| DEFAULT_BOOTSTRAP_ROOT.to_string());
    let bootstrap_root = bootstrap_root.trim_end_matches('/').to_string();
    let mut notes = Vec::new();
    let mut warnings = Vec::new();

    let username = config
        .username()
        .context("Guest.PrimaryUser.Username must be set (`username:` in vm.config.local.yaml)")?;
    let hash = password_hash(config)?;
    let key_source = plans::key_source(config)?;
    warnings.extend(
        key_source
            .material
            .warnings
            .iter()
            .map(|w| format!("ssh_keys: {w}")),
    );

    let mut payload: Vec<Payload> = Vec::new();
    let mut add = |target: &str, permissions: &str, contents: Vec<u8>| {
        payload.push(Payload {
            target: target.to_string(),
            permissions: permissions.to_string(),
            contents,
        })
    };

    add(
        "/etc/vm-bootstrap.env",
        "0600",
        unix_text(&bootstrap_env(
            config,
            target,
            &key_source,
            &bootstrap_root,
        )?),
    );
    add(
        "/etc/vm-bootstrap.users",
        "0600",
        unix_text(&users_file(config)?),
    );
    add(
        "/etc/vm-bootstrap.gitconfig-extra",
        "0644",
        unix_text(&git_extra_file(config)?),
    );
    add(
        "/etc/vm-bootstrap.disks",
        "0644",
        unix_text(&disks_file(config)?),
    );
    add(
        "/etc/vm-bootstrap.repos",
        "0644",
        unix_text(&repos_file(config)?),
    );
    add(
        "/etc/vm-bootstrap.files",
        "0644",
        unix_text(&files_manifest(config)?),
    );

    let copies = plans::file_copies(config)?;
    warnings.extend(copies.warnings.clone());
    let mut copied = 0;
    for entry in &copies.entries {
        for file in &entry.files {
            let bytes = std::fs::read(&file.source)
                .with_context(|| format!("could not read {}", file.source.display()))?;
            add(
                &format!("{}/{}", entry.stage_root, file.relative),
                &file.permissions,
                bytes,
            );
            copied += 1;
        }
    }
    if copied > 0 {
        notes.push(format!(
            "copying {copied} file(s) from {} folder(s)",
            copies.entries.len()
        ));
    }

    let hook = hook_script(config)?;
    if !hook.is_empty() {
        add(
            &format!("{bootstrap_root}/hooks/post-start.sh"),
            "0755",
            unix_text(&hook),
        );
    }

    for secret in secrets(config, &key_source)? {
        if secret.secret {
            warnings.push(format!(
                "embedding a secret in the seed disk: {}",
                secret.label
            ));
        }
        add(&secret.target, &secret.permissions, secret.contents);
    }

    //The bootstrap tree: the runner and its unit go to fixed places, every other
    //file under the bootstrap root with its layout intact.
    let tree = &blueprint.bootstrap;
    for required in ["_runner.sh", "_vm-bootstrap.service"] {
        if !tree.contains_key(required) {
            bail!(
                "blueprint '{}' has no {required} in any of its bootstrap folders",
                blueprint.name
            );
        }
    }
    let mut plain_copy: Vec<(String, Vec<u8>)> = Vec::new();
    for (relative, asset) in tree {
        let bytes = asset.read()?;
        let bytes = if relative.ends_with(".sh") || relative.ends_with(".service") {
            unix_text(&String::from_utf8_lossy(&bytes))
        } else {
            bytes
        };
        match relative.as_str() {
            "_runner.sh" => add("/usr/local/sbin/vm-bootstrap", "0755", bytes),
            "_vm-bootstrap.service" => {
                add("/etc/systemd/system/vm-bootstrap.service", "0644", bytes)
            }
            _ => {
                let permissions = if relative.ends_with(".sh") {
                    "0755"
                } else {
                    "0644"
                };
                plain_copy.push((format!("bootstrap/{relative}"), bytes.clone()));
                add(&format!("{bootstrap_root}/{relative}"), permissions, bytes);
            }
        }
    }

    //Authorized keys: config, the drop folder, and Vmseed's own.
    let mut keys: Vec<String> = string_list(config.get("Guest.PrimaryUser.AuthorizedKeys"))
        .into_iter()
        .map(|k| k.trim().to_string())
        .collect();
    keys.extend(key_source.material.authorized_keys.iter().cloned());
    if let Some(own) = own_key {
        keys.push(own.to_string());
    }
    let mut unique: Vec<String> = Vec::new();
    for key in keys {
        if !key.is_empty() && !unique.contains(&key) {
            unique.push(key);
        }
    }
    let keys = unique;

    let method = blueprint
        .get_string("Provisioning.Method")
        .unwrap_or_else(|| "cloud-init-nocloud".to_string());
    let kickstart = match method.as_str() {
        "kickstart-oemdrv" => true,
        "cloud-init-nocloud" => false,
        other => bail!(
            "blueprint '{}': unsupported Provisioning.Method '{other}'. Use cloud-init-nocloud or kickstart-oemdrv.",
            blueprint.name
        ),
    };
    //Hyper-V's guest daemons only make sense on Hyper-V.
    let mut packages: Vec<String> = string_list(blueprint.get("Packages"));
    if target.hypervisor == HypervisorKind::HyperV {
        packages.extend(string_list(blueprint.get("Provisioning.HyperVPackages")));
    }

    let (package_block, keys_block, write_files) = if kickstart {
        let keys_block = keys
            .iter()
            .map(|key| format!("sshkey --username={username} \"{key}\""))
            .collect::<Vec<_>>()
            .join("\n");
        let files = payload
            .iter()
            .map(shell_write_file)
            .collect::<Vec<_>>()
            .join("\n\n");
        (packages.join("\n"), keys_block, files)
    } else {
        let package_block = if packages.is_empty() {
            "    []".to_string()
        } else {
            packages
                .iter()
                .map(|p| format!("    - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let keys_block = if keys.is_empty() {
            String::new()
        } else {
            let mut lines = vec!["    authorized-keys:".to_string()];
            lines.extend(keys.iter().map(|k| format!("      - {k}")));
            lines.join("\n")
        };
        let files = payload
            .iter()
            .map(write_file_entry)
            .collect::<Vec<_>>()
            .join("\n");
        (package_block, keys_block, files)
    };

    let real_name = config
        .string("Guest.PrimaryUser.RealName")
        .unwrap_or_else(|| username.clone());
    let release = config.string_or("Iso.Release", "");
    let install_source = blueprint
        .get_string("Provisioning.InstallSource")
        .map(|s| s.replace("{Release}", &release))
        .unwrap_or_default();
    let shutdown = if blueprint.get_bool("Provisioning.PowerOffAfterInstall", true) {
        "poweroff"
    } else {
        "reboot"
    };
    let stamp = humantime::format_rfc3339_seconds(std::time::SystemTime::now())
        .to_string()
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    let tokens: Vec<(&str, String)> = vec![
        ("LOCALE", config.string_or("Guest.Locale", "en_US.UTF-8")),
        ("TIMEZONE", config.string_or("Guest.Timezone", "UTC")),
        (
            "KEYBOARD_LAYOUT",
            config.string_or("Guest.KeyboardLayout", "us"),
        ),
        (
            "SOURCE_ID",
            blueprint
                .get_string("Provisioning.SourceId")
                .unwrap_or_else(|| "ubuntu-server".to_string()),
        ),
        ("HOSTNAME", config.hostname.clone()),
        ("REALNAME", real_name.replace('"', "\\\"")),
        ("USERNAME", username.clone()),
        ("PASSWORD_HASH", hash),
        (
            "SSH_INSTALL_SERVER",
            shell_bool(config.flag("Guest.Ssh.InstallServer", true)).to_string(),
        ),
        (
            "SSH_ALLOW_PW",
            shell_bool(config.flag("Guest.Ssh.AllowPasswordAuth", true)).to_string(),
        ),
        ("SSH_AUTHORIZED_KEYS", keys_block),
        ("PACKAGES", package_block),
        ("WRITE_FILES", write_files),
        ("SHUTDOWN_MODE", shutdown.to_string()),
        ("INSTANCE_ID", format!("iid-{}-{stamp}", config.vm_name)),
        ("SYSTEM_DISK", target.system_disk.clone()),
        ("INSTALL_SOURCE", install_source),
    ];

    let seed_name = blueprint
        .get_string("Provisioning.SeedFileName")
        .unwrap_or_else(|| "user-data".to_string());
    let mut templates = vec![(seed_name.clone(), blueprint.seed_template.read_text()?)];
    if let Some(meta) = &blueprint.meta_template {
        templates.push(("meta-data".to_string(), meta.read_text()?));
    }
    let token_shape = regex_lite::Regex::new(r"\{\{([A-Z_]+)\}\}").expect("valid");
    let mut seed_files = Vec::new();
    for (name, template) in templates {
        let mut text = template;
        for (token, value) in &tokens {
            text = text.replace(&format!("{{{{{token}}}}}"), value);
        }
        if let Some(found) = token_shape.captures(&text) {
            bail!(
                "unreplaced template token {{{{{}}}}} in {name}: the blueprint template expects a value {} does not supply",
                &found[1],
                crate::brand::DISPLAY_NAME
            );
        }
        seed_files.push((
            name,
            String::from_utf8(unix_text(&text)).expect("utf-8 in, utf-8 out"),
        ));
    }

    let label = blueprint
        .get_string("Provisioning.SeedLabel")
        .unwrap_or_else(|| "CIDATA".to_string());
    let disk_files: Vec<(String, Vec<u8>)> = seed_files
        .iter()
        .map(|(name, text)| (name.clone(), text.clone().into_bytes()))
        .collect();
    let mut review_files = disk_files.clone();
    review_files.extend(plain_copy);
    notes.push(format!("ssh key for GitHub: {}", key_source.origin));
    if !key_source.material.authorized_keys.is_empty() {
        notes.push(format!(
            "ssh_keys: {} public key(s) added to the guest's authorized_keys",
            key_source.material.authorized_keys.len()
        ));
    }

    Ok(Seed {
        method,
        label,
        seed_files,
        disk_files,
        review_files,
        payload_count: payload.len(),
        notes,
        warnings,
    })
}

//One file as a cloud-init write_files entry, base64 so quoting and line
//endings can't corrupt it.
fn write_file_entry(file: &Payload) -> String {
    let mut lines = vec![
        format!("      - path: {}", file.target),
        "        owner: root:root".to_string(),
        format!("        permissions: '{}'", file.permissions),
        "        encoding: b64".to_string(),
        "        content: |".to_string(),
    ];
    lines.extend(
        base64_lines(&file.contents)
            .into_iter()
            .map(|l| format!("          {l}")),
    );
    lines.join("\n")
}

//One file as shell commands that recreate it, for kickstart's %post.
fn shell_write_file(file: &Payload) -> String {
    let parent = file.target.rsplit_once('/').map(|(p, _)| p).unwrap_or("/");
    let mut lines = vec![
        format!("install -d -m 0755 '{parent}'"),
        format!("base64 -d > '{}' <<'VMBOOTSTRAP_B64'", file.target),
    ];
    lines.extend(base64_lines(&file.contents));
    lines.push("VMBOOTSTRAP_B64".to_string());
    lines.push(format!("chown root:root '{}'", file.target));
    lines.push(format!("chmod {} '{}'", file.permissions, file.target));
    lines.join("\n")
}

//---------------------------------------------------------------- generated files

///etc/vm-bootstrap.env: the one interface between config and the bash steps.
fn bootstrap_env(
    config: &Effective,
    target: &Target,
    keys: &KeySource,
    bootstrap_root: &str,
) -> Result<String> {
    let b = |path: &str, default: bool| shell_bool(config.flag(path, default)).to_string();
    let s = |path: &str, default: &str| config.string_or(path, default);

    let mut seen: Vec<String> = Vec::new();
    let mut extensions: Vec<String> = Vec::new();
    for id in config
        .list("Bootstrap.VSCodeExtensions")
        .into_iter()
        .chain(config.list("Bootstrap.ExtraVSCodeExtensions"))
    {
        let id = id.trim().to_string();
        if !id.is_empty() && !seen.contains(&id.to_lowercase()) {
            seen.push(id.to_lowercase());
            extensions.push(id);
        }
    }
    let db = plans::database(config)?;
    let rh = plans::red_hat(config)?;
    let hooks = plans::hooks(config)?;
    let release = s("Iso.Release", "");
    let rhsm_release = if rh.pin_release
        && regex_lite::Regex::new(r"^\d+\.\d+$")
            .expect("valid")
            .is_match(&release)
    {
        release.clone()
    } else {
        String::new()
    };
    let key_type = if keys.key_type.is_empty() {
        s("GitHub.SshKey.Type", "ed25519")
    } else {
        keys.key_type.clone()
    };

    let vars: Vec<(&str, String)> = vec![
        ("VM_BOOTSTRAP_ROOT", bootstrap_root.to_string()),
        ("VM_BLUEPRINT", config.blueprint.name.clone()),
        ("VM_HYPERVISOR", target.hypervisor.as_str().to_string()),
        ("VM_ARCH", config.arch.as_str().to_string()),
        ("VM_HOSTNAME", config.hostname.clone()),
        ("VM_PRIMARY_USER", config.username().unwrap_or_default()),
        ("VM_TIMEZONE", s("Guest.Timezone", "")),
        ("VM_LOCALE", s("Guest.Locale", "")),
        ("VM_KEYBOARD_LAYOUT", s("Guest.KeyboardLayout", "us")),
        ("VM_SSH_INSTALL_SERVER", b("Guest.Ssh.InstallServer", true)),
        (
            "VM_SSH_ALLOW_PASSWORD_AUTH",
            b("Guest.Ssh.AllowPasswordAuth", true),
        ),
        ("VM_ENABLE_MDNS", b("Bootstrap.EnableMdns", true)),
        ("VM_MDNS_PEERS", config.peers.join(" ")),
        ("VM_ENHANCED_SESSION", b("VM.EnhancedSession", true)),
        ("VM_INSTALL_DESKTOP", b("Bootstrap.InstallDesktop", true)),
        ("VM_DESKTOP_PACKAGE", s("Bootstrap.DesktopPackage", "")),
        (
            "VM_DISPLAY_RESOLUTION",
            s("Bootstrap.DisplayResolution", "1920x1080"),
        ),
        ("VM_INSTALL_DOCKER", b("Bootstrap.InstallDocker", true)),
        ("VM_INSTALL_DDEV", b("Bootstrap.InstallDdev", false)),
        ("VM_INSTALL_NODE", b("Bootstrap.InstallNode", true)),
        ("VM_NODE_VERSION", s("Bootstrap.NodeVersion", "lts/*")),
        ("VM_PYTHON_VERSION", s("Bootstrap.PythonVersion", "3.14")),
        ("VM_NVM_VERSION", s("Bootstrap.NvmVersion", "v0.40.1")),
        ("VM_INSTALL_PYTHON", b("Bootstrap.InstallPython", true)),
        ("VM_INSTALL_VSCODE", b("Bootstrap.InstallVSCode", true)),
        ("VM_INSTALL_CHROME", b("Bootstrap.InstallChrome", true)),
        ("VM_INSTALL_POSTMAN", b("Bootstrap.InstallPostman", false)),
        ("VM_VSCODE_EXTENSIONS", extensions.join(" ")),
        (
            "VM_INSTALL_REMOTE_ACCESS",
            b("Bootstrap.InstallRemoteAccess", true),
        ),
        (
            "VM_EXTRA_APT_PACKAGES",
            config.list("Bootstrap.ExtraAptPackages").join(" "),
        ),
        ("VM_DOTFILES_REPO", s("Bootstrap.DotfilesRepo", "")),
        ("VM_REPOS_ROOT", plans::repo_root(config)?),
        ("VM_REBOOT_WHEN_DONE", b("Bootstrap.RebootWhenDone", true)),
        ("VM_GIT_USER_NAME", s("Git.UserName", "")),
        ("VM_GIT_USER_EMAIL", s("Git.UserEmail", "")),
        ("VM_GIT_DEFAULT_BRANCH", s("Git.DefaultBranch", "")),
        ("VM_GIT_EDITOR", s("Git.Editor", "")),
        ("VM_GIT_PULL_REBASE", b("Git.PullRebase", false)),
        (
            "VM_DB_ENGINE",
            db.as_ref().map(|d| d.engine.clone()).unwrap_or_default(),
        ),
        (
            "VM_DB_VERSION",
            db.as_ref().map(|d| d.version.clone()).unwrap_or_default(),
        ),
        (
            "VM_DB_SOURCE",
            db.as_ref().map(|d| d.source.clone()).unwrap_or_default(),
        ),
        (
            "VM_DB_PORT",
            db.as_ref().map(|d| d.port.to_string()).unwrap_or_default(),
        ),
        (
            "VM_DB_LISTEN_ADDRESS",
            db.as_ref()
                .map(|d| d.listen_address.clone())
                .unwrap_or_default(),
        ),
        (
            "VM_DB_INSTALL_CLIENT",
            shell_bool(db.as_ref().is_some_and(|d| d.install_client)).to_string(),
        ),
        (
            "VM_DB_INSTALL_SERVER",
            shell_bool(db.as_ref().is_some_and(|d| d.install_server)).to_string(),
        ),
        (
            "VM_DB_SELINUX_TYPE",
            db.as_ref()
                .map(|d| d.selinux_type.clone())
                .unwrap_or_default(),
        ),
        (
            "VM_DB_DATABASES",
            db.as_ref()
                .map(|d| d.databases.join(" "))
                .unwrap_or_default(),
        ),
        (
            "VM_ADMINER_INSTALL",
            shell_bool(db.as_ref().is_some_and(|d| d.adminer_install)).to_string(),
        ),
        (
            "VM_ADMINER_PORT",
            db.as_ref()
                .map(|d| d.adminer_port.to_string())
                .unwrap_or_default(),
        ),
        (
            "VM_ADMINER_VERSION",
            db.as_ref()
                .map(|d| d.adminer_version.clone())
                .unwrap_or_default(),
        ),
        (
            "VM_ADMINER_PHP_DRIVER_APT",
            db.as_ref()
                .map(|d| d.php_driver_apt.clone())
                .unwrap_or_default(),
        ),
        (
            "VM_ADMINER_PHP_DRIVER_DNF",
            db.as_ref()
                .map(|d| d.php_driver_dnf.clone())
                .unwrap_or_default(),
        ),
        ("VM_RHSM_MODE", rh.mode.clone()),
        ("VM_RHSM_ORG_ID", rh.organization_id.clone()),
        ("VM_RHSM_USERNAME", rh.username.clone()),
        (
            "VM_RHSM_AUTO_ATTACH",
            shell_bool(rh.auto_attach).to_string(),
        ),
        ("VM_RHSM_RELEASE", rhsm_release),
        (
            "VM_HOOKS_RUN_AS",
            hooks.as_ref().map(|h| h.run_as.clone()).unwrap_or_default(),
        ),
        (
            "VM_HOOKS_WORKING_DIR",
            hooks
                .as_ref()
                .map(|h| h.working_directory.clone())
                .unwrap_or_default(),
        ),
        (
            "VM_HOOKS_EVERY_BOOT",
            shell_bool(hooks.as_ref().is_some_and(|h| h.every_boot)).to_string(),
        ),
        (
            "VM_HOOKS_SCRIPT",
            hooks
                .as_ref()
                .map(|_| format!("{bootstrap_root}/hooks/post-start.sh"))
                .unwrap_or_default(),
        ),
        ("VM_SECRETS_DIR", SECRETS_DIR.to_string()),
        ("VM_GITHUB_USERNAME", s("GitHub.Username", "")),
        ("VM_GITHUB_SSH_KEY_MODE", keys.mode.clone()),
        ("VM_GITHUB_SSH_KEY_TYPE", key_type),
        ("VM_GITHUB_SSH_KEY_COMMENT", s("GitHub.SshKey.Comment", "")),
        ("VM_GITHUB_KNOWN_HOSTS", b("GitHub.KnownHosts", true)),
        ("VM_GITHUB_INSTALL_CLI", b("GitHub.Cli.Install", false)),
        ("VM_GITHUB_CLI_PROTOCOL", s("GitHub.Cli.Protocol", "ssh")),
        ("VM_GITHUB_UPLOAD_SSH_KEY", b("GitHub.UploadSshKey", false)),
        (
            "VM_GITHUB_SHRED_SECRETS",
            b("GitHub.ShredSecretsAfterUse", true),
        ),
    ];
    let mut text = format!(
        "# GENERATED by {} - edit vm.config.yaml, not this file.\n# Sourced by /usr/local/sbin/vm-bootstrap with `set -a`.\n",
        crate::brand::NAME
    );
    for (key, value) in vars {
        text.push_str(&format!("{key}={}\n", shell_quote(&value)));
    }
    Ok(text)
}

//Tab separated, read by 05-users.sh.
fn users_file(config: &Effective) -> Result<String> {
    let mut text =
        "# username<TAB>realname<TAB>hash<TAB>sudo<TAB>groups<TAB>base64(authorized_keys)\n"
            .to_string();
    let Some(serde_norway::Value::Sequence(users)) = config.get("Guest.AdditionalUsers") else {
        return Ok(text);
    };
    for user in users {
        let Some(map) = user.as_mapping() else {
            bail!("each guest.additionalUsers entry must be a block with username");
        };
        let get = |key: &str| crate::config::get_string(map, key);
        let Some(username) = get("Username") else {
            bail!("every entry in guest.additionalUsers needs a username");
        };
        let real_name = get("RealName").unwrap_or_else(|| username.clone());
        let hash = match (get("PasswordHash"), get("Password")) {
            (Some(hash), _) => {
                if !hash.starts_with('$') {
                    bail!("guest user '{username}': passwordHash does not look like a crypt hash");
                }
                passwd::to_glibc_form(&hash)
            }
            (None, Some(password)) => passwd::hash_password(&password)?,
            (None, None) => "-".to_string(),
        };
        let sudo = if crate::config::get_path(map, "Sudo").is_some_and(crate::config::is_truthy) {
            "1"
        } else {
            "0"
        };
        let groups = string_list(crate::config::get_path(map, "Groups"));
        let groups = if groups.is_empty() {
            "-".to_string()
        } else {
            groups.join(",")
        };
        let keys = string_list(crate::config::get_path(map, "AuthorizedKeys"));
        let keys = if keys.is_empty() {
            "-".to_string()
        } else {
            BASE64.encode(format!("{}\n", keys.join("\n")))
        };
        text.push_str(&[username.as_str(), &real_name, &hash, sudo, &groups, &keys].join("\t"));
        text.push('\n');
    }
    Ok(text)
}

fn git_extra_file(config: &Effective) -> Result<String> {
    let mut text = "# section.key<TAB>value - applied with `git config --global`\n".to_string();
    let extra = config.section("Git.Extra");
    let key_shape = regex_lite::Regex::new(r"^[A-Za-z0-9]+([.-][A-Za-z0-9_-]+)+$").expect("valid");
    let mut pairs: Vec<(String, String)> = extra
        .iter()
        .filter_map(|(k, v)| {
            let value = match v {
                serde_norway::Value::String(s) => s.clone(),
                serde_norway::Value::Number(n) => n.to_string(),
                serde_norway::Value::Bool(b) => b.to_string(),
                _ => return None,
            };
            Some((k.as_str()?.to_string(), value))
        })
        .collect();
    pairs.sort();
    for (key, value) in pairs {
        if !key_shape.is_match(&key) {
            bail!(
                "git.extra key '{key}' is not a git config key. Use 'section.key' form, for example 'push.autoSetupRemote'."
            );
        }
        if value.contains('\t') || value.contains('\n') {
            bail!(
                "git.extra['{key}'] contains a tab or newline, which this file format cannot carry"
            );
        }
        text.push_str(&format!("{key}\t{value}\n"));
    }
    Ok(text)
}

fn disks_file(config: &Effective) -> Result<String> {
    let mut text = "# label<TAB>scsi-lun<TAB>mountpoint<TAB>filesystem<TAB>sizeGB\n".to_string();
    for disk in plans::data_disks(config)? {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            disk.label, disk.location, disk.mount_point, disk.filesystem, disk.size_gb
        ));
    }
    Ok(text)
}

fn repos_file(config: &Effective) -> Result<String> {
    let mut text =
        "# url<TAB>directory<TAB>branch<TAB>depth<TAB>recurse<TAB>base64(post-clone command)\n"
            .to_string();
    for repo in plans::repos(config)? {
        let post = if repo.post_clone.is_empty() {
            "-".to_string()
        } else {
            BASE64.encode(repo.post_clone.as_bytes())
        };
        let branch = if repo.branch.is_empty() {
            "-"
        } else {
            &repo.branch
        };
        text.push_str(&format!(
            "{}\t{}\t{branch}\t{}\t{}\t{post}\n",
            repo.url,
            repo.directory,
            repo.depth,
            if repo.recurse { "1" } else { "0" }
        ));
    }
    Ok(text)
}

fn files_manifest(config: &Effective) -> Result<String> {
    let mut text = "# stage<TAB>destination<TAB>owner<TAB>mode<TAB>filecount\n# mode is \"-\" when no explicit Mode was given.\n".to_string();
    for entry in plans::file_copies(config)?.entries {
        //A literal '-' rather than an empty field: `IFS=$'\t' read` collapses
        //empty fields and would shift every field after it.
        let mode = if entry.mode.is_empty() {
            "-".to_string()
        } else {
            entry.mode.clone()
        };
        text.push_str(&format!(
            "{}\t{}\t{}\t{mode}\t{}\n",
            entry.stage_root,
            entry.to,
            entry.owner,
            entry.files.len()
        ));
    }
    Ok(text)
}

fn hook_script(config: &Effective) -> Result<String> {
    let Some(plan) = plans::hooks(config)? else {
        return Ok(String::new());
    };
    let mut text = format!(
        "#!/usr/bin/env bash\n#\n# GENERATED by {} from hooks in vm.config.yaml.\n# Edit the config and rebuild, or re-run this file directly to test a change:\n#\n#     sudo vm-bootstrap --only 80-hooks --force\n#\nset -euo pipefail\n\n",
        crate::brand::NAME
    );
    if !plan.commands.is_empty() {
        text.push_str("# ---- hooks.postStart -------------------------------------------------\n");
        for command in &plan.commands {
            text.push_str(command);
            text.push('\n');
        }
        text.push('\n');
    }
    if let Some(file) = &plan.script_file {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        text.push_str(&format!("# ---- hooks.postStartFile: {name} ----\n"));
        let body = std::fs::read_to_string(file)
            .with_context(|| format!("could not read {}", file.display()))?;
        for (index, line) in body.lines().enumerate() {
            if index == 0 && line.starts_with("#!") {
                continue;
            }
            text.push_str(line);
            text.push('\n');
        }
    }
    Ok(text)
}

struct Secret {
    target: String,
    permissions: String,
    contents: Vec<u8>,
    secret: bool,
    label: String,
}

//The things on the seed disk worth protecting. Each lands 0600 under /root and
//is shredded by bootstrap once used.
fn secrets(config: &Effective, keys: &KeySource) -> Result<Vec<Secret>> {
    let mut out = Vec::new();
    let mut add =
        |name: &str, permissions: &str, contents: Vec<u8>, secret: bool, label: String| {
            out.push(Secret {
                target: format!("{SECRETS_DIR}/{name}"),
                permissions: permissions.to_string(),
                contents,
                secret,
                label,
            })
        };
    if let Some(db) = plans::database(config)?
        && !db.admin_password.is_empty()
    {
        add(
            "db-admin-password",
            "0600",
            db.admin_password.clone().into_bytes(),
            true,
            format!("{} admin password", db.display_name),
        );
    }
    let rh = plans::red_hat(config)?;
    if !rh.secret.is_empty() {
        let label = if rh.mode == "activation-key" {
            "Red Hat activation key".to_string()
        } else {
            format!("Red Hat account password for {}", rh.username)
        };
        add(
            "redhat-subscription-secret",
            "0600",
            rh.secret.clone().into_bytes(),
            true,
            label,
        );
    }
    if keys.mode == "copy"
        && let Some(private) = &keys.private_key
    {
        let bytes = std::fs::read(private)
            .with_context(|| format!("could not read {}", private.display()))?;
        add(
            "github-ssh-key",
            "0600",
            bytes,
            true,
            format!("private SSH key from {}", private.display()),
        );
        if !keys.public_text.is_empty() {
            add(
                "github-ssh-key.pub",
                "0644",
                format!("{}\n", keys.public_text).into_bytes(),
                false,
                "public SSH key".to_string(),
            );
        }
    }
    let token = match (
        config.string("GitHub.Cli.TokenFile"),
        config.string("GitHub.Cli.Token"),
    ) {
        (Some(file), _) => {
            let path = config.config_path(&file);
            let token = std::fs::read_to_string(&path)
                .with_context(|| format!("github.cli.tokenFile not found: {}", path.display()))?
                .trim()
                .to_string();
            if token.is_empty() {
                bail!("github.cli.tokenFile is empty: {}", path.display());
            }
            Some((token, path.display().to_string()))
        }
        (None, Some(token)) => Some((token, "github.cli.token in config".to_string())),
        _ => None,
    };
    if let Some((token, origin)) = token {
        if token.chars().any(char::is_whitespace) {
            bail!("the GitHub token contains whitespace. Check the file holds only the token.");
        }
        add(
            "github-token",
            "0600",
            token.into_bytes(),
            true,
            format!("GitHub token from {origin}"),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand;
    use crate::constants::files;
    use serde_norway::Mapping;

    fn config(yaml: &str) -> (Effective, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "vmseed-seed-render-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let folder = root.join(brand::DIR);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(files::CONFIG_FILE), yaml).unwrap();
        (Effective::load(&root, None, Mapping::new()).unwrap(), root)
    }

    fn target(kind: HypervisorKind) -> Target {
        Target {
            hypervisor: kind,
            system_disk: "sda".to_string(),
        }
    }

    #[test]
    fn kickstart_seeds_render_completely() {
        let (config, root) = config(
            "blueprint: redhat-rocky-workstation\nprojectName: acme\nusername: dev\npasswordHash: '$6$salt$hash'\ndbEngine: postgres\ndbServer: true\n",
        );
        let seed = render(
            &config,
            &target(HypervisorKind::Libvirt),
            Some("ssh-ed25519 AAAA vmseed@test"),
        )
        .unwrap();
        assert_eq!(seed.label, "OEMDRV");
        let (name, ks) = &seed.seed_files[0];
        assert_eq!(name, "ks.cfg");
        assert!(ks.contains(
            "network --bootproto=dhcp --activate --hostname=acme-redhat-rocky-workstation"
        ));
        assert!(ks.contains("sshkey --username=dev \"ssh-ed25519 AAAA vmseed@test\""));
        assert!(ks.contains("--password=\"$6$salt$hash\" --iscrypted"));
        assert!(ks.contains("base64 -d > '/etc/vm-bootstrap.env'"));
        assert!(ks.contains("ignoredisk --only-use=sda"));
        assert!(!ks.contains("{{"));
        assert!(!ks.contains("hyperv-daemons"), "only on Hyper-V");
        assert!(
            seed.review_files
                .iter()
                .any(|(n, _)| n == "bootstrap/01-base.sh")
        );
        assert!(
            seed.disk_files.iter().all(|(n, _)| !n.contains('/')),
            "the seed volume is flat"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cloud_init_seeds_render_and_carry_the_env() {
        //arch is pinned: left on auto it follows the computer running the
        //tests, and an Apple Silicon Mac would test the arm64 package list.
        //hypervisor is fake because the Mac's own one (vfkit) refuses a guest
        //of another architecture; the seed is still rendered for Hyper-V.
        let (config, root) = config(
            "blueprint: ubuntu-server\narch: x86_64\nhypervisor: fake\nusername: dev\npassword: secret\n",
        );
        let seed = render(&config, &target(HypervisorKind::HyperV), None).unwrap();
        assert_eq!(seed.label, "CIDATA");
        assert_eq!(seed.seed_files.len(), 2);
        let user_data = &seed.seed_files[0].1;
        assert!(user_data.starts_with("#cloud-config"));
        assert!(user_data.contains("      - path: /etc/vm-bootstrap.env"));
        assert!(
            user_data.contains("linux-cloud-tools-virtual"),
            "Hyper-V gets its guest tools"
        );
        let meta = &seed.seed_files[1].1;
        assert!(meta.contains("local-hostname: dev-ubuntu-server"));
        //The env file names the hypervisor, decoded back out of the payload.
        let env = bootstrap_env(
            &config,
            &target(HypervisorKind::HyperV),
            &plans::key_source(&config).unwrap(),
            DEFAULT_BOOTSTRAP_ROOT,
        )
        .unwrap();
        assert!(env.contains("VM_HYPERVISOR='hyperv'\n"));
        assert!(env.contains("VM_INSTALL_DESKTOP='false'\n"));
        std::fs::remove_dir_all(root).unwrap();
    }

    //Ubuntu builds linux-cloud-tools for amd64 only.
    #[test]
    fn arm64_hyper_v_guests_get_no_cloud_tools() {
        let (arm, arm_root) = config(
            "blueprint: ubuntu-server\narch: arm64\nhypervisor: fake\nusername: dev\npassword: secret\n",
        );
        let arm_seed = render(&arm, &target(HypervisorKind::HyperV), None).unwrap();
        assert!(
            !arm_seed.seed_files[0]
                .1
                .contains("linux-cloud-tools-virtual"),
            "arm64 has no Hyper-V guest tools package"
        );
        std::fs::remove_dir_all(arm_root).unwrap();
    }

    #[test]
    fn shell_quoting_survives_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
