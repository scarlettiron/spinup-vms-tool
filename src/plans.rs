//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The parts of a config that need working out before they mean anything: data
//disks, the database, files to copy, Red Hat registration, hooks, repositories
//and SSH keys. Each is checked here, so a typo fails in a second rather than
//half an hour into a build.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use regex_lite::Regex;
use serde_norway::{Mapping, Value};

use crate::brand;
use crate::config::effective::{Effective, expand_env, string_list};
use crate::config::{get_path, get_string, is_truthy, value_as_u64};
use crate::constants::files;

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("the pattern is valid")
}

fn text(map: &Mapping, key: &str) -> String {
    get_string(map, key).unwrap_or_default()
}

fn flag(map: &Mapping, key: &str) -> bool {
    get_path(map, key).is_some_and(is_truthy)
}

//---------------------------------------------------------------- disk layout

//Disk positions on the VM's SCSI controller. The guest finds its data disks by
//these numbers, so they are fixed rather than left to the hypervisor.
pub const LOCATION_OS: u32 = 0;
pub const LOCATION_DVD: u32 = 1;
pub const LOCATION_SEED: u32 = 2;
pub const LOCATION_DATA: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDisk {
    pub label: String,
    pub size_gb: u64,
    pub mount_point: String,
    pub filesystem: String,
    pub location: u32,
    pub drive_letter: String,
}

pub fn data_disks(config: &Effective) -> Result<Vec<DataDisk>> {
    let Some(Value::Sequence(items)) = config.get("VM.DataDisks") else {
        return Ok(Vec::new());
    };
    let mut layout: Vec<DataDisk> = Vec::new();
    for (location, item) in (LOCATION_DATA..).zip(items.iter()) {
        let Value::Mapping(disk) = item else {
            bail!(
                "each entry in vm.dataDisks must be a block, for example {{ label: data-e, driveLetter: E, sizeGB: 50 }}"
            );
        };
        let label = text(disk, "Label");
        let size_gb = get_path(disk, "SizeGB").and_then(value_as_u64).unwrap_or(0);
        if label.is_empty() {
            bail!("a vm.dataDisks entry is missing label");
        }
        if size_gb == 0 {
            bail!("a vm.dataDisks entry is missing sizeGB");
        }
        if label.len() > 16 {
            bail!(
                "data disk label '{label}' is longer than 16 characters, which ext4 cannot store"
            );
        }
        if !regex("^[A-Za-z0-9_-]+$").is_match(&label) {
            bail!("data disk label '{label}' must be letters, digits, hyphen, or underscore");
        }
        if layout.iter().any(|d| d.label == label) {
            bail!("two vm.dataDisks entries share the label '{label}'");
        }

        let mut letter = text(disk, "DriveLetter")
            .trim_end_matches(':')
            .to_uppercase();
        if !letter.is_empty() {
            if !regex("^[A-Z]$").is_match(&letter) {
                bail!("data disk '{label}' driveLetter must be a single letter (got '{letter}')");
            }
            if letter == "C" {
                bail!(
                    "data disk '{label}' cannot use drive letter C; that is the system disk (set diskGB for its size)"
                );
            }
            if layout.iter().any(|d| d.drive_letter == letter) {
                bail!("two vm.dataDisks entries use drive letter {letter}");
            }
        }
        let mount = match get_string(disk, "MountPoint") {
            Some(mount) => mount.trim_end_matches('/').to_string(),
            None if !letter.is_empty() => format!("/mnt/{}", letter.to_lowercase()),
            None => bail!("data disk '{label}' needs either a driveLetter or a mountPoint"),
        };
        if mount.is_empty() {
            bail!("data disk '{label}' cannot mount at /; that is the system disk");
        }
        if !mount.starts_with('/') {
            bail!("data disk '{label}' mountPoint must be an absolute Linux path (got '{mount}')");
        }
        if layout.iter().any(|d| d.mount_point == mount) {
            bail!("two vm.dataDisks entries mount at '{mount}'");
        }
        let filesystem = get_string(disk, "Filesystem").unwrap_or_else(|| "ext4".to_string());
        if filesystem != "ext4" && filesystem != "xfs" {
            bail!("data disk '{label}' filesystem must be ext4 or xfs (got '{filesystem}')");
        }
        letter.truncate(1);
        layout.push(DataDisk {
            label,
            size_gb,
            mount_point: mount,
            filesystem,
            location,
            drive_letter: letter,
        });
    }
    Ok(layout)
}

//---------------------------------------------------------------- database

struct Engine {
    name: &'static str,
    display: &'static str,
    default_version: &'static str,
    default_port: u16,
    versions: &'static [&'static str],
    source: &'static str,
    selinux_type: &'static str,
    php_apt: &'static str,
    php_dnf: &'static str,
}

const ENGINES: &[Engine] = &[
    Engine {
        name: "mysql",
        display: "MySQL",
        default_version: "8",
        default_port: 3306,
        versions: &["8", "8.0", "8.4"],
        source: "distro",
        selinux_type: "mysqld_port_t",
        php_apt: "php-mysql",
        php_dnf: "php-mysqlnd",
    },
    Engine {
        name: "postgres",
        display: "PostgreSQL",
        default_version: "17",
        default_port: 5432,
        versions: &["13", "14", "15", "16", "17", "18"],
        source: "pgdg",
        selinux_type: "postgresql_port_t",
        php_apt: "php-pgsql",
        php_dnf: "php-pgsql",
    },
];

const ADMINER_DEFAULT_VERSION: &str = "4.8.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabasePlan {
    pub engine: String,
    pub display_name: String,
    pub version: String,
    pub port: u16,
    pub listen_address: String,
    pub source: String,
    pub selinux_type: String,
    pub install_client: bool,
    pub install_server: bool,
    pub admin_password: String,
    pub databases: Vec<String>,
    pub remote: bool,
    pub adminer_install: bool,
    pub adminer_port: u16,
    pub adminer_version: String,
    pub php_driver_apt: String,
    pub php_driver_dnf: String,
}

fn engine_names() -> String {
    ENGINES
        .iter()
        .map(|e| e.name)
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn database(config: &Effective) -> Result<Option<DatabasePlan>> {
    let db = config.section("Database");
    let mut engine = text(&db, "Engine").to_lowercase();
    let want_client = flag(&db, "InstallClient");
    let want_server = flag(&db, "InstallServer");
    let adminer_install = flag(&db, "Adminer.Install");

    if engine.is_empty() {
        if want_client || want_server {
            bail!(
                "database.installClient or database.installServer is set but database.engine is blank. \
                 Set dbEngine to one of: {}.",
                engine_names()
            );
        }
        if adminer_install {
            bail!(
                "database.adminer.install is set but database.engine is blank. Adminer needs an engine to know \
                 which driver to load: set dbEngine to one of: {}.",
                engine_names()
            );
        }
        return Ok(None);
    }
    engine = match engine.as_str() {
        "postgresql" | "pgsql" | "pg" => "postgres".to_string(),
        "mariadb" => "mysql".to_string(),
        other => other.to_string(),
    };
    let Some(spec) = ENGINES.iter().find(|e| e.name == engine) else {
        bail!(
            "unknown database.engine '{engine}'. Supported engines are: {}.",
            engine_names()
        );
    };
    if !want_client && !want_server && !adminer_install {
        bail!(
            "database.engine is '{engine}' but none of installClient, installServer or adminer.install is set, \
             so nothing would be installed. Set dbClient, dbServer or dbAdminer to true, or blank dbEngine."
        );
    }
    let version = get_string(&db, "Version").unwrap_or_else(|| spec.default_version.to_string());
    if !spec.versions.contains(&version.as_str()) {
        bail!(
            "database.version '{version}' is not a supported {} version. Choose one of: {}.",
            spec.display,
            spec.versions.join(", ")
        );
    }
    let port = match get_path(&db, "Port").and_then(value_as_u64) {
        Some(0) | None => u64::from(spec.default_port),
        Some(port) => port,
    };
    if !(1..=65535).contains(&port) {
        bail!("database.port '{port}' is outside 1-65535");
    }
    let listen = get_string(&db, "ListenAddress").unwrap_or_else(|| "localhost".to_string());
    if listen != "localhost" && listen != "*" {
        bail!("database.listenAddress must be 'localhost' or '*', not '{listen}'");
    }
    let mut databases = Vec::new();
    for name in string_list(get_path(&db, "Databases")) {
        let name = name.trim().to_string();
        if !regex("^[A-Za-z_][A-Za-z0-9_]*$").is_match(&name) {
            bail!(
                "database name '{name}' must start with a letter or underscore and contain only letters, digits and underscores"
            );
        }
        databases.push(name);
    }
    let admin_password = text(&db, "AdminPassword");
    if want_server && listen == "*" && admin_password.is_empty() {
        bail!(
            "database.listenAddress is '*' but database.adminPassword is blank, which would expose the {} superuser \
             to every machine on the network. Set adminPassword, or leave listenAddress as 'localhost'.",
            spec.display
        );
    }
    let adminer_port = match get_path(&db, "Adminer.Port").and_then(value_as_u64) {
        Some(0) | None => u64::from(crate::constants::config_keys::DEFAULT_ADMINER_PORT),
        Some(port) => port,
    };
    let adminer_version =
        get_string(&db, "Adminer.Version").unwrap_or_else(|| ADMINER_DEFAULT_VERSION.to_string());
    if adminer_install {
        if !(1..=65535).contains(&adminer_port) {
            bail!("database.adminer.port '{adminer_port}' is outside 1-65535");
        }
        if adminer_port == port {
            bail!(
                "database.adminer.port and database.port are both {port}. Adminer needs a port of its own."
            );
        }
        if !regex(r"^\d+\.\d+(\.\d+)?$").is_match(&adminer_version) {
            bail!(
                "database.adminer.version '{adminer_version}' does not look like a release number, for example 4.8.1"
            );
        }
    }
    Ok(Some(DatabasePlan {
        engine,
        display_name: spec.display.to_string(),
        version,
        port: port as u16,
        remote: listen == "*",
        listen_address: listen,
        source: spec.source.to_string(),
        selinux_type: spec.selinux_type.to_string(),
        install_client: want_client || want_server,
        install_server: want_server,
        admin_password,
        databases,
        adminer_install,
        adminer_port: adminer_port as u16,
        adminer_version,
        php_driver_apt: spec.php_apt.to_string(),
        php_driver_dnf: spec.php_dnf.to_string(),
    }))
}

//---------------------------------------------------------------- files

const FILE_COPY_WARN_BYTES: u64 = 10 << 20;
const FILE_COPY_MAX_BYTES: u64 = 64 << 20;

#[derive(Debug, Clone)]
pub struct CopyFile {
    pub source: PathBuf,
    pub relative: String,
    pub size: u64,
    pub permissions: String,
}

#[derive(Debug, Clone)]
pub struct CopyEntry {
    pub from: PathBuf,
    pub to: String,
    pub owner: String,
    pub mode: String,
    pub files: Vec<CopyFile>,
    pub size: u64,
    pub stage_root: String,
}

pub struct FileCopyLayout {
    pub entries: Vec<CopyEntry>,
    pub warnings: Vec<String>,
}

pub fn file_copies(config: &Effective) -> Result<FileCopyLayout> {
    let section = config.section("Files");
    let mut raw: Vec<Mapping> = Vec::new();
    if get_string(&section, "Source").is_some() {
        let mut shorthand = Mapping::new();
        for (from, to) in [
            ("Source", "From"),
            ("Destination", "To"),
            ("Owner", "Owner"),
            ("Mode", "Mode"),
        ] {
            if let Some(value) = get_path(&section, from) {
                shorthand.insert(Value::from(to), value.clone());
            }
        }
        raw.push(shorthand);
    }
    match get_path(&section, "Copy") {
        Some(Value::Sequence(items)) => {
            for item in items {
                let Value::Mapping(map) = item else {
                    bail!(
                        "each entry in files.copy must be a block, for example {{ from: files/certs, to: /usr/local/share/ca-certificates }}"
                    );
                };
                raw.push(map.clone());
            }
        }
        None => {}
        Some(_) => bail!("files.copy must be a list"),
    }

    let primary = config.username().unwrap_or_default();
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut total: u64 = 0;
    for entry in raw {
        let Some(from_text) = get_string(&entry, "From") else {
            bail!("a files.copy entry is missing from");
        };
        let from = config.config_path(&from_text);
        if !from.exists() {
            bail!("files.copy source not found: {}", from.display());
        }
        if !from.is_dir() {
            bail!(
                "files.copy from must be a folder, not a file: {}. Point it at the folder holding the files.",
                from.display()
            );
        }
        let mut to = get_string(&entry, "To")
            .unwrap_or_else(|| "~".to_string())
            .replace('\\', "/");
        if to != "~" && !to.starts_with("~/") && !to.starts_with('/') {
            bail!("files.copy to must be an absolute guest path or start with '~', not '{to}'");
        }
        to = to.trim_end_matches('/').to_string();
        if to.is_empty() {
            to = "/".to_string();
        }
        let owner = get_string(&entry, "Owner").unwrap_or_else(|| primary.clone());
        if !regex("^[A-Za-z_][A-Za-z0-9_-]*(:[A-Za-z_][A-Za-z0-9_-]*)?$").is_match(&owner) {
            bail!("files.copy owner '{owner}' is not a valid user or user:group");
        }
        let mode = get_string(&entry, "Mode").unwrap_or_default();
        if !mode.is_empty() && !regex("^[0-7]{3,4}$").is_match(&mode) {
            bail!("files.copy mode '{mode}' must be an octal permission such as 0644");
        }
        let mut found = Vec::new();
        walk(&from, &mut found)?;
        found.sort();
        let mut copy_files = Vec::new();
        for path in found {
            let size = std::fs::metadata(&path)?.len();
            total += size;
            let relative = path
                .strip_prefix(&from)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            let permissions = if !mode.is_empty() {
                mode.clone()
            } else if path.extension().is_some_and(|e| e == "sh") {
                "0755".to_string()
            } else {
                "0644".to_string()
            };
            copy_files.push(CopyFile {
                source: path,
                relative,
                size,
                permissions,
            });
        }
        if copy_files.is_empty() {
            warnings.push(format!(
                "files.copy source '{}' has no files in it; nothing to copy",
                from.display()
            ));
            continue;
        }
        let index = entries.len();
        let size = copy_files.iter().map(|f| f.size).sum();
        entries.push(CopyEntry {
            from,
            to,
            owner,
            mode,
            files: copy_files,
            size,
            stage_root: format!("/var/lib/vm-bootstrap/files/{index}"),
        });
    }
    if total > FILE_COPY_MAX_BYTES {
        bail!(
            "files.copy totals {:.1} MB, over the {} MB limit. Everything here is embedded in the seed disk, \
             so this is for config and certificates, not bulk data.",
            total as f64 / (1 << 20) as f64,
            FILE_COPY_MAX_BYTES >> 20
        );
    }
    if total > FILE_COPY_WARN_BYTES {
        warnings.push(format!(
            "files.copy totals {:.1} MB, which slows the install: it is base64-encoded into the seed",
            total as f64 / (1 << 20) as f64
        ));
    }
    Ok(FileCopyLayout { entries, warnings })
}

fn walk(dir: &Path, into: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("could not read {}", dir.display()))?
    {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, into)?;
        } else {
            into.push(path);
        }
    }
    Ok(())
}

//---------------------------------------------------------------- Red Hat

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedHatPlan {
    //none, activation-key or credentials
    pub mode: String,
    pub organization_id: String,
    pub username: String,
    pub auto_attach: bool,
    pub pin_release: bool,
    //The activation key or password; staged as a private file, never in the env.
    pub secret: String,
}

pub fn red_hat(config: &Effective) -> Result<RedHatPlan> {
    let rh = config.section("RedHat");
    let mut activation_key = text(&rh, "ActivationKey");
    let org_id = text(&rh, "OrganizationId");
    let username = text(&rh, "Username");
    let password = text(&rh, "Password");
    if let Some(key_file) = get_string(&rh, "ActivationKeyFile")
        && activation_key.is_empty()
    {
        let path = config.config_path(&key_file);
        activation_key = std::fs::read_to_string(&path)
            .with_context(|| {
                format!(
                    "redHat.activationKeyFile points at a file that can't be read: {}",
                    path.display()
                )
            })?
            .trim()
            .to_string();
    }
    let auto_attach = get_path(&rh, "AutoAttach").map(is_truthy).unwrap_or(true);
    let pin_release = get_path(&rh, "PinRelease").map(is_truthy).unwrap_or(true);
    let mode = if !activation_key.is_empty() && !org_id.is_empty() {
        "activation-key"
    } else if !username.is_empty() && !password.is_empty() {
        "credentials"
    } else {
        if !activation_key.is_empty() {
            bail!(
                "redHat.activationKey is set but redHat.organizationId is not. An activation key only works with its \
                 organization id, shown as the Organization ID on console.redhat.com."
            );
        }
        if !org_id.is_empty() {
            bail!("redHat.organizationId is set but redHat.activationKey is not");
        }
        if !username.is_empty() {
            bail!(
                "redHat.username is set but redHat.password is not. Prefer an activation key: it carries no password \
                 and can be revoked on its own."
            );
        }
        if !password.is_empty() {
            bail!("redHat.password is set but redHat.username is not");
        }
        "none"
    };
    let secret = match mode {
        "activation-key" => activation_key,
        "credentials" => password,
        _ => String::new(),
    };
    Ok(RedHatPlan {
        mode: mode.to_string(),
        organization_id: org_id,
        username,
        auto_attach,
        pin_release,
        secret,
    })
}

//---------------------------------------------------------------- hooks

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPlan {
    pub commands: Vec<String>,
    pub script_file: Option<PathBuf>,
    pub run_as: String,
    pub working_directory: String,
    pub every_boot: bool,
}

pub fn hooks(config: &Effective) -> Result<Option<HookPlan>> {
    let section = config.section("Hooks");
    let mut commands = Vec::new();
    for line in string_list(get_path(&section, "PostStart")) {
        //A multi-line string is a block of commands.
        commands.extend(line.lines().map(|l| l.trim_end_matches('\r').to_string()));
    }
    let script_file = match get_string(&section, "PostStartFile") {
        Some(file) => {
            let path = config.config_path(&file);
            if !path.is_file() {
                bail!("hooks.postStartFile not found: {}", path.display());
            }
            Some(path)
        }
        None => None,
    };
    let run_as = get_string(&section, "RunAs")
        .unwrap_or_else(|| "user".to_string())
        .to_lowercase();
    if run_as != "user" && run_as != "root" {
        bail!("hooks.runAs must be 'user' or 'root', not '{run_as}'");
    }
    let working_directory = text(&section, "WorkingDirectory").replace('\\', "/");
    if !working_directory.is_empty()
        && working_directory != "~"
        && !working_directory.starts_with("~/")
        && !working_directory.starts_with('/')
    {
        bail!(
            "hooks.workingDirectory must be an absolute guest path or start with '~', not '{working_directory}'"
        );
    }
    let every_boot = flag(&section, "RunAtEveryBoot");
    if commands.is_empty() && script_file.is_none() {
        if every_boot {
            bail!(
                "hooks.runAtEveryBoot is set but there are no hooks to run. Add commands to hooks.postStart, \
                 or point hooks.postStartFile at a script."
            );
        }
        return Ok(None);
    }
    Ok(Some(HookPlan {
        commands,
        script_file,
        run_as,
        working_directory,
        every_boot,
    }))
}

//---------------------------------------------------------------- repos

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub url: String,
    pub directory: String,
    pub branch: String,
    pub depth: u64,
    pub recurse: bool,
    pub post_clone: String,
    pub needs_ssh: bool,
}

pub fn repos(config: &Effective) -> Result<Vec<Repo>> {
    let Some(Value::Sequence(items)) = config.get("Repos.Items") else {
        return Ok(Vec::new());
    };
    let url_shape = regex(r"^(https?://|ssh://|git://|file://|[A-Za-z0-9._-]+@[A-Za-z0-9._-]+:)");
    let ssh_shape = regex(r"^(ssh://|[A-Za-z0-9._-]+@[A-Za-z0-9._-]+:)");
    let mut layout: Vec<Repo> = Vec::new();
    for item in items {
        let entry = match item {
            Value::Mapping(map) => map.clone(),
            Value::String(url) => {
                let mut map = Mapping::new();
                map.insert(Value::from("Url"), Value::from(url.clone()));
                map
            }
            _ => bail!("each repos entry must be a URL or a block with url"),
        };
        let url = text(&entry, "Url");
        if url.is_empty() {
            bail!("a repos entry has no url");
        }
        if !url_shape.is_match(&url) {
            bail!(
                "repos entry '{url}' does not look like a git URL. Expected https://, ssh://, git://, file://, or user@host:path."
            );
        }
        let directory = match get_string(&entry, "Directory") {
            Some(dir) => dir,
            None => {
                let trimmed = url.trim_end_matches('/');
                let leaf = trimmed.rsplit(['/', ':']).next().unwrap_or("");
                leaf.trim_end_matches(".git").to_string()
            }
        }
        .replace('\\', "/");
        if directory.is_empty() {
            bail!("could not work out a folder name for '{url}'. Set directory explicitly.");
        }
        if directory.starts_with('/') || regex(r"(^|/)\.\.(/|$)").is_match(&directory) {
            bail!(
                "repos entry '{url}': directory must be a relative path without '..' (got '{directory}')"
            );
        }
        if layout.iter().any(|r| r.directory == directory) {
            bail!(
                "two repos entries would both clone into '{directory}'. Set directory on one of them."
            );
        }
        layout.push(Repo {
            needs_ssh: ssh_shape.is_match(&url),
            branch: text(&entry, "Branch"),
            depth: get_path(&entry, "Depth")
                .and_then(value_as_u64)
                .unwrap_or(0),
            recurse: flag(&entry, "Recurse"),
            post_clone: text(&entry, "PostClone"),
            url,
            directory,
        });
    }
    Ok(layout)
}

pub fn repo_root(config: &Effective) -> Result<String> {
    let root = config.string_or("Repos.Root", "src").replace('\\', "/");
    let root = root.trim_end_matches('/');
    let root = if root.is_empty() { "src" } else { root };
    if regex(r"(^|/)\.\.(/|$)").is_match(root) {
        bail!("repos.root must not contain '..' (got '{root}')");
    }
    Ok(root.to_string())
}

//---------------------------------------------------------------- ssh keys

#[derive(Debug, Clone)]
pub struct PrivateKey {
    pub name: String,
    pub path: PathBuf,
    pub public_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct KeyMaterial {
    pub private_keys: Vec<PrivateKey>,
    pub identity: Option<PrivateKey>,
    pub authorized_keys: Vec<String>,
    pub warnings: Vec<String>,
    pub ambiguity: Option<String>,
}

//What is in the project's .vmseed/ssh_keys/ drop folder.
pub fn key_material(config: &Effective, prefer: &str) -> Result<KeyMaterial> {
    let dir = config
        .project_root
        .join(brand::DIR)
        .join(files::SSH_KEYS_DIR);
    let mut result = KeyMaterial::default();
    if !dir.is_dir() {
        return Ok(result);
    }
    let skip = [
        "README.md",
        ".gitignore",
        ".gitkeep",
        "known_hosts",
        "config",
        ".DS_Store",
        "Thumbs.db",
    ];
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    names.sort();

    let mut privates = Vec::new();
    let mut publics: Vec<(String, PathBuf, String)> = Vec::new();
    for path in names {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if skip.contains(&name.as_str()) {
            continue;
        }
        if path.extension().is_some_and(|e| e == "ppk") {
            result.warnings.push(format!(
                "{name} is a PuTTY key, which OpenSSH cannot read. Convert it: PuTTYgen, then Conversions, then Export OpenSSH key."
            ));
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            result.warnings.push(format!("could not read {name}"));
            continue;
        };
        let first = content.lines().next().unwrap_or("").trim().to_string();
        if name == "authorized_keys" {
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    result.authorized_keys.push(line.to_string());
                }
            }
        } else if regex("^-----BEGIN .*PRIVATE KEY-----").is_match(&first) {
            privates.push((name, path));
        } else if regex("^(ssh-|ecdsa-|sk-)").is_match(&first) {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            publics.push((stem, path, first));
        } else {
            result.warnings.push(format!(
                "ignoring {name}: it is neither an OpenSSH private key nor a public key"
            ));
        }
    }

    for (name, path) in privates {
        let paired = publics.iter().find(|(stem, _, _)| *stem == name);
        let public_text = match paired {
            Some((_, _, text)) => text.clone(),
            None => match crate::guest::keys::public_from_private(&path) {
                Some(Some(text)) => text,
                Some(None) => {
                    result.warnings.push(format!(
                        "{name} looks passphrase-protected and has no {name}.pub beside it. Drop the .pub file in too; \
                         otherwise an unattended clone during the first boot will fail."
                    ));
                    String::new()
                }
                None => {
                    result
                        .warnings
                        .push(format!("no ssh-keygen found to derive a public key for {name}. Drop {name}.pub in as well."));
                    String::new()
                }
            },
        };
        result.private_keys.push(PrivateKey {
            name,
            path,
            public_text,
        });
    }

    let names: Vec<String> = result.private_keys.iter().map(|k| k.name.clone()).collect();
    if result.private_keys.len() == 1 {
        result.identity = result.private_keys.first().cloned();
    } else if result.private_keys.len() > 1 {
        if !prefer.is_empty() {
            match result.private_keys.iter().find(|k| k.name == prefer) {
                Some(key) => result.identity = Some(key.clone()),
                None => {
                    result.ambiguity = Some(format!(
                        "github.sshKey.name is '{prefer}' but ssh_keys/ holds: {}",
                        names.join(", ")
                    ))
                }
            }
        } else {
            let conventional = regex("^id_(ed25519|rsa|ecdsa)$");
            match result
                .private_keys
                .iter()
                .find(|k| conventional.is_match(&k.name))
            {
                Some(key) => result.identity = Some(key.clone()),
                None => {
                    result.ambiguity = Some(format!(
                        "ssh_keys/ holds more than one private key ({}) and none is named id_ed25519, id_rsa, or id_ecdsa. \
                         Set github.sshKey.name to the one GitHub should use.",
                        names.join(", ")
                    ))
                }
            }
        }
    }
    //Every public key in the folder lets its owner log in.
    for (_, _, text) in publics {
        result.authorized_keys.push(text);
    }
    result.authorized_keys.dedup();
    Ok(result)
}

//The key's algorithm, from its public half: "ssh-ed25519 AAAA..." is ed25519.
pub fn key_type(public_text: &str) -> String {
    let algorithm = public_text.split_whitespace().next().unwrap_or("");
    match algorithm {
        "ssh-ed25519" => "ed25519",
        "ssh-rsa" => "rsa",
        a if a.starts_with("sk-ssh-ed25519") => "ed25519_sk",
        a if a.starts_with("sk-ecdsa-sha2-") => "ecdsa_sk",
        a if a.starts_with("ecdsa-sha2-") => "ecdsa",
        _ => "",
    }
    .to_string()
}

#[derive(Debug, Clone)]
pub struct KeySource {
    //none, generate or copy: never "auto", which is resolved here.
    pub mode: String,
    pub private_key: Option<PathBuf>,
    pub public_text: String,
    pub key_type: String,
    //Where it came from, in words.
    pub origin: String,
    pub material: KeyMaterial,
}

pub fn key_source(config: &Effective) -> Result<KeySource> {
    let ssh = config.section("GitHub.SshKey");
    let mode = get_string(&ssh, "Mode")
        .unwrap_or_else(|| "auto".to_string())
        .to_lowercase();
    if !["auto", "none", "generate", "copy"].contains(&mode.as_str()) {
        bail!("github.sshKey.mode must be 'auto', 'none', 'generate', or 'copy' (got '{mode}')");
    }
    let prefer = text(&ssh, "Name");
    let material = key_material(config, &prefer)?;
    let mut source = KeySource {
        mode: mode.clone(),
        private_key: None,
        public_text: String::new(),
        key_type: String::new(),
        origin: String::new(),
        material,
    };
    if mode == "none" {
        source.origin = "disabled".to_string();
        return Ok(source);
    }
    if let Some(explicit) = get_string(&ssh, "PrivateKeyPath")
        && mode != "generate"
    {
        let path = config.config_path(&expand_env(&explicit));
        if !path.is_file() {
            bail!("github.sshKey.privateKeyPath not found: {}", path.display());
        }
        let public_path = get_string(&ssh, "PublicKeyPath")
            .map(|p| config.config_path(&p))
            .unwrap_or_else(|| PathBuf::from(format!("{}.pub", path.display())));
        source.public_text = if public_path.is_file() {
            std::fs::read_to_string(&public_path)?
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        } else {
            crate::guest::keys::public_from_private(&path)
                .flatten()
                .unwrap_or_default()
        };
        source.mode = "copy".to_string();
        source.key_type = key_type(&source.public_text);
        source.origin = format!("github.sshKey.privateKeyPath ({})", path.display());
        source.private_key = Some(path);
        return Ok(source);
    }
    if mode == "generate" {
        source.origin = "generated in the guest".to_string();
        return Ok(source);
    }
    if let Some(message) = &source.material.ambiguity {
        bail!("{message}");
    }
    if let Some(identity) = source.material.identity.clone() {
        source.mode = "copy".to_string();
        source.public_text = identity.public_text.clone();
        source.key_type = key_type(&identity.public_text);
        source.origin = format!("ssh_keys/{}", identity.name);
        source.private_key = Some(identity.path);
        return Ok(source);
    }
    if mode == "copy" {
        bail!(
            "github.sshKey.mode is 'copy' but no private key was found. Drop one into .vmseed/ssh_keys/, set privateKeyPath, \
             or use mode 'auto'."
        );
    }
    source.mode = "generate".to_string();
    source.origin = "generated in the guest (ssh_keys/ has no private key)".to_string();
    Ok(source)
}

//---------------------------------------------------------------- everything

//Runs every check, so `config --validate` and `create` fail on the same things.
pub fn check_all(config: &Effective) -> Result<Vec<String>> {
    let mut warnings = config.warnings.clone();
    data_disks(config)?;
    database(config)?;
    warnings.extend(file_copies(config)?.warnings);
    red_hat(config)?;
    hooks(config)?;
    repos(config)?;
    repo_root(config)?;
    let keys = key_source(config)?;
    warnings.extend(
        keys.material
            .warnings
            .iter()
            .map(|w| format!("ssh_keys: {w}")),
    );
    let repos = repos(config)?;
    let ssh_repos = repos.iter().filter(|r| r.needs_ssh).count();
    if ssh_repos > 0 && keys.mode == "none" {
        warnings.push(format!(
            "{ssh_repos} repo(s) use an ssh remote but github.sshKey.mode is 'none'; those clones will fail"
        ));
    } else if ssh_repos > 0 && keys.mode == "generate" {
        let token = config.string("GitHub.Cli.Token").is_some()
            || config.string("GitHub.Cli.TokenFile").is_some();
        if !(token && config.flag("GitHub.UploadSshKey", false)) {
            warnings.push(format!(
                "{ssh_repos} repo(s) use an ssh remote and the key is generated inside the guest, so it is not on \
                 GitHub yet; private clones will fail on the first boot. Supply githubTokenFile with uploadSshKey, \
                 or use https:// remotes."
            ));
        }
    }
    if config.blueprint.get_string("Os.Vendor").as_deref() == Some("rhel")
        && red_hat(config)?.mode == "none"
    {
        warnings.push(
            "no Red Hat subscription is configured, so this VM will have NO package repositories: bootstrap can add \
             nothing (no EPEL, so no remote desktop). Set redHat.activationKey and redHat.organizationId in \
             vm.config.local.yaml."
                .to_string(),
        );
    }
    Ok(warnings)
}

//The checks only a build needs: someone to log in as, with a password.
//`config --validate` reports these as warnings, since a fresh project often
//has no user yet.
pub fn check_user(config: &Effective) -> Result<()> {
    if config.username().is_none() {
        bail!(
            "no username is set. Add `username:` and `passwordHash:` to .vmseed/{} (make a hash with `{} hash`).",
            files::LOCAL_CONFIG_FILE,
            brand::NAME
        );
    }
    let has_password = config.string("Guest.PrimaryUser.Password").is_some()
        || config.string("Guest.PrimaryUser.PasswordHash").is_some();
    if !has_password {
        bail!(
            "the user has no password: the installers will not create a passwordless account. Add `passwordHash:` to \
             .vmseed/{} (make one with `{} hash`).",
            files::LOCAL_CONFIG_FILE,
            brand::NAME
        );
    }
    if let Some(hash) = config.string("Guest.PrimaryUser.PasswordHash")
        && !regex(r"^\$[0-9a-z]+\$").is_match(&hash)
    {
        bail!(
            "passwordHash does not look like a crypt hash. Make one with `{} hash`.",
            brand::NAME
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(yaml: &str) -> (Effective, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "vmseed-plans-{}-{}",
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

    #[test]
    fn data_disks_get_locations_and_mount_points() {
        let (config, root) = load(
            "blueprint: ubuntu-server\nvm:\n  dataDisks:\n    - { label: drive-e, driveLetter: e, sizeGB: 50 }\n    - { label: logs, mountPoint: /var/log/app/, sizeGB: 5 }\n",
        );
        let disks = data_disks(&config).unwrap();
        assert_eq!(disks[0].mount_point, "/mnt/e");
        assert_eq!(disks[0].drive_letter, "E");
        assert_eq!(disks[0].location, LOCATION_DATA);
        assert_eq!(disks[1].mount_point, "/var/log/app");
        assert_eq!(disks[1].location, LOCATION_DATA + 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn database_plans_fill_defaults_and_refuse_nonsense() {
        let (config, root) =
            load("blueprint: ubuntu-server\ndbEngine: pg\ndbServer: true\ndbAdminer: true\n");
        let plan = database(&config).unwrap().unwrap();
        assert_eq!(plan.engine, "postgres");
        assert_eq!(plan.version, "17");
        assert_eq!(plan.port, 5432);
        assert_eq!(plan.adminer_port, 3800);
        assert!(plan.install_client);
        std::fs::remove_dir_all(root).unwrap();

        let (config, root) =
            load("blueprint: ubuntu-server\ndbEngine: postgres\ndbVersion: '9'\ndbClient: true\n");
        assert!(
            database(&config)
                .unwrap_err()
                .to_string()
                .contains("not a supported")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repos_take_their_folder_from_the_url() {
        let (config, root) = load(
            "blueprint: ubuntu-server\nrepos: [git@github.com:acme/web.git, 'https://example.com/x/api']\n",
        );
        let repos = repos(&config).unwrap();
        assert_eq!(repos[0].directory, "web");
        assert!(repos[0].needs_ssh);
        assert_eq!(repos[1].directory, "api");
        assert!(!repos[1].needs_ssh);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn key_types_come_from_the_public_key() {
        assert_eq!(key_type("ssh-ed25519 AAAA x"), "ed25519");
        assert_eq!(key_type("ecdsa-sha2-nistp256 AAAA"), "ecdsa");
        assert_eq!(key_type("nonsense"), "");
    }
}
