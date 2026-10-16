//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The complete config for building one VM: every layer merged, the blueprint
//resolved, names worked out and checked. Layers, lowest to highest:
//
//  1. built-in defaults (assets/defaults.yaml)
//  2. the blueprint's Defaults
//  3. .vmseed/vm.config.yaml
//  4. the blueprint's Overrides (what the OS *is*, above the shared file)
//  5. .vmseed/vm.config.local.yaml
//  6. the chosen Machines: entry
//  7. command-line overrides

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use regex_lite::Regex;
use serde_norway::{Mapping, Value};

use super::{
    display_name, expand_aliases, get_path, get_string, is_truthy, load_layer, merge_into,
    remove_key, set_path, value_as_u64,
};
use crate::blueprint::resolve::{Resolved, resolve};
use crate::brand;
use crate::constants::config_keys::{DEFAULT_PROJECT_NAME, MAX_HOSTNAME_LENGTH};
use crate::constants::files;
use crate::platform::{Arch, HypervisorKind};
use crate::utilities::paths;

const BUILT_IN_DEFAULTS: &str = include_str!("../../assets/defaults.yaml");

#[derive(Debug, Clone)]
pub struct Effective {
    pub project_root: PathBuf,
    pub machine: Option<String>,
    pub blueprint: Resolved,
    //Every setting, merged.
    pub raw: Mapping,
    pub vm_name: String,
    pub hostname: String,
    pub arch: Arch,
    pub hypervisor: HypervisorKind,
    //Every hostname in the project's fleet, so guests can check their mDNS peers.
    pub peers: Vec<String>,
    //Things worth saying that are not errors.
    pub warnings: Vec<String>,
}

impl Effective {
    //Loads the config for the project's VM, or one of its machines.
    pub fn load(project_root: &Path, machine: Option<&str>, overrides: Mapping) -> Result<Self> {
        let folder = project_root.join(brand::DIR);
        let shared_path = folder.join(files::CONFIG_FILE);
        let local_path = folder.join(files::LOCAL_CONFIG_FILE);
        if !shared_path.is_file() {
            bail!(
                "{} not found. Run `{} init` in the project folder first.",
                paths::display_path(&shared_path),
                brand::NAME
            );
        }

        let mut shared = expand_aliases(load_layer(&shared_path)?, &display_name(&shared_path))?;
        let mut local = if local_path.is_file() {
            expand_aliases(load_layer(&local_path)?, &display_name(&local_path))?
        } else {
            Mapping::new()
        };
        let mut warnings = Vec::new();
        if get_path(&shared, "Guest.PrimaryUser.Password").is_some() {
            warnings.push(format!(
                "a plain password is set in {} (the committed file). Everyone with the repository can read it; \
                 move it to {} or use passwordHash.",
                files::CONFIG_FILE,
                files::LOCAL_CONFIG_FILE
            ));
        }

        //The Machines block may be in either file; it is a menu, not a setting.
        let mut machines = Mapping::new();
        for layer in [&mut shared, &mut local] {
            if let Some(Value::Mapping(block)) = remove_key(layer, "Machines") {
                merge_into(&mut machines, block);
            }
        }

        let block = match machine {
            None => Mapping::new(),
            Some(wanted) => {
                let entry = machines.iter().find(|(key, _)| {
                    key.as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
                });
                match entry {
                    Some((_, Value::Mapping(map))) => {
                        expand_aliases(map.clone(), &format!("machine '{wanted}'"))?
                    }
                    Some((_, Value::Null)) => Mapping::new(),
                    Some(_) => bail!("Machines.{wanted} must be a block of settings"),
                    None => {
                        let mut known: Vec<String> = machine_names(&machines);
                        known.sort();
                        let known = if known.is_empty() {
                            "(none defined)".to_string()
                        } else {
                            known.join(", ")
                        };
                        bail!("unknown machine '{wanted}'. Defined machines: {known}");
                    }
                }
            }
        };

        //Which blueprint, arch and hypervisor: the highest layer that names one.
        let mut probe = shared.clone();
        for layer in [&local, &block, &overrides] {
            merge_into(&mut probe, layer.clone());
        }
        let blueprint_name =
            get_string(&probe, "Blueprint").unwrap_or_else(|| files::DEFAULT_BLUEPRINT.to_string());
        let arch = Arch::from_setting(get_string(&probe, "Arch").as_deref())?;
        let hypervisor = HypervisorKind::from_setting(get_string(&probe, "Hypervisor").as_deref())?;
        if hypervisor == HypervisorKind::Vfkit && arch != Arch::host() {
            bail!(
                "arch is {} but this Mac is {}; Apple Virtualization only runs guests of the host's architecture",
                arch.as_str(),
                Arch::host().as_str()
            );
        }

        let blueprint = resolve(&blueprint_name, Some(project_root), arch)?;

        let mut effective: Mapping = serde_norway::from_str(BUILT_IN_DEFAULTS)
            .context("the built-in defaults are not valid YAML")?;
        fill_machine_defaults(&mut effective);
        for path in ["Defaults", "", "Overrides"] {
            let layer = if path.is_empty() {
                shared.clone()
            } else {
                match blueprint.get(path) {
                    Some(Value::Mapping(map)) => map.clone(),
                    _ => Mapping::new(),
                }
            };
            merge_into(&mut effective, layer);
        }
        for layer in [local, block.clone(), overrides.clone()] {
            merge_into(&mut effective, layer);
        }
        set_path(
            &mut effective,
            "Blueprint",
            Value::String(blueprint.name.clone()),
        );

        //A machine's key is its name unless its own block (or the command line)
        //says otherwise. The local file is deliberately not consulted: a single
        //VmName there would collapse a whole fleet onto one name.
        if let Some(wanted) = machine {
            let named = |path: &str| {
                get_path(&block, path).is_some() || get_path(&overrides, path).is_some()
            };
            if !named("VM.Name") {
                set_path(&mut effective, "VM.Name", Value::String(wanted.to_string()));
            }
            if !named("Guest.Hostname") {
                set_path(
                    &mut effective,
                    "Guest.Hostname",
                    Value::String(wanted.to_string()),
                );
            }
        }

        //Every hostname in the fleet.
        let mut peers: Vec<String> = Vec::new();
        let mut keys = machine_names(&machines);
        keys.sort();
        for key in keys {
            let entry = machines
                .iter()
                .find(|(k, _)| k.as_str() == Some(key.as_str()))
                .map(|(_, v)| v.clone());
            let host = entry
                .as_ref()
                .and_then(Value::as_mapping)
                .and_then(|map| expand_aliases(map.clone(), "machine").ok())
                .and_then(|map| get_string(&map, "Guest.Hostname"));
            peers.push(host.unwrap_or(key));
        }

        //The OS version: configured, or the blueprint's own.
        let release = match get_string(&effective, "Iso.Release") {
            Some(release) => release,
            None => match blueprint.get_string("Os.Version") {
                Some(version) => {
                    set_path(
                        &mut effective,
                        "Iso.Release",
                        Value::String(version.clone()),
                    );
                    version
                }
                None => bail!(
                    "blueprint '{}' declares no Os.Version and no osVersion is configured; set one",
                    blueprint.name
                ),
            },
        };
        if let Some(pattern) = blueprint.get_string("Os.VersionPattern") {
            let regex = Regex::new(&pattern).with_context(|| {
                format!("blueprint '{}' has a bad Os.VersionPattern", blueprint.name)
            })?;
            if !regex.is_match(&release) {
                let own = blueprint
                    .get_string("Os.Version")
                    .map(|v| format!(" Its own version is {v}."))
                    .unwrap_or_default();
                bail!(
                    "osVersion '{release}' does not look like a {} release.{own} Set osVersion to match the blueprint, \
                     or remove it to use the blueprint's own.",
                    blueprint.name
                );
            }
        }

        //Names: <projectName>-<blueprint> unless set.
        let project_name = get_string(&effective, "ProjectName")
            .unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_string());
        if !Regex::new("^[A-Za-z0-9][A-Za-z0-9-]*$")
            .expect("valid")
            .is_match(&project_name)
        {
            bail!(
                "projectName '{project_name}' must be letters, digits and hyphens, starting with a letter or digit. \
                 It becomes part of a hostname."
            );
        }
        let derived = format!("{project_name}-{}", blueprint.name).to_lowercase();
        let vm_name = get_string(&effective, "VM.Name").unwrap_or_else(|| derived.clone());
        let hostname = get_string(&effective, "Guest.Hostname").unwrap_or(derived);
        set_path(&mut effective, "VM.Name", Value::String(vm_name.clone()));
        set_path(
            &mut effective,
            "Guest.Hostname",
            Value::String(hostname.clone()),
        );
        if hostname.len() > MAX_HOSTNAME_LENGTH {
            bail!(
                "hostname '{hostname}' is {} characters; DNS labels stop at {MAX_HOSTNAME_LENGTH}. \
                 Shorten projectName, or set hostname explicitly.",
                hostname.len()
            );
        }
        if !Regex::new("^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?$")
            .expect("valid")
            .is_match(&hostname)
        {
            bail!(
                "hostname '{hostname}' is not a valid DNS label. Use letters, digits and hyphens only, \
                 not starting or ending with a hyphen."
            );
        }
        if !Regex::new("^[A-Za-z0-9][A-Za-z0-9._-]*$")
            .expect("valid")
            .is_match(&vm_name)
        {
            bail!("vmName '{vm_name}' must be letters, digits, dots, hyphens and underscores");
        }
        if peers.is_empty() {
            peers.push(hostname.clone());
        }

        Ok(Effective {
            project_root: project_root.to_path_buf(),
            machine: machine.map(str::to_string),
            blueprint,
            raw: effective,
            vm_name,
            hostname,
            arch,
            hypervisor,
            peers,
            warnings,
        })
    }

    pub fn get(&self, path: &str) -> Option<&Value> {
        get_path(&self.raw, path)
    }

    pub fn string(&self, path: &str) -> Option<String> {
        get_string(&self.raw, path)
    }

    pub fn string_or(&self, path: &str, default: &str) -> String {
        self.string(path).unwrap_or_else(|| default.to_string())
    }

    pub fn flag(&self, path: &str, default: bool) -> bool {
        self.get(path).map(is_truthy).unwrap_or(default)
    }

    pub fn number(&self, path: &str, default: u64) -> u64 {
        self.get(path).and_then(value_as_u64).unwrap_or(default)
    }

    //A block of settings, or an empty one.
    pub fn section(&self, path: &str) -> Mapping {
        match self.get(path) {
            Some(Value::Mapping(map)) => map.clone(),
            _ => Mapping::new(),
        }
    }

    //A list setting as text items. A single value counts as a list of one.
    pub fn list(&self, path: &str) -> Vec<String> {
        string_list(self.get(path))
    }

    //Where this VM's files live on this computer.
    pub fn vm_dir(&self) -> PathBuf {
        PathBuf::from(self.string_or("VM.Path", ".")).join(&self.vm_name)
    }

    pub fn username(&self) -> Option<String> {
        self.string("Guest.PrimaryUser.Username")
    }

    pub fn url(&self) -> String {
        format!("http://{}.local", self.hostname)
    }

    //Relative paths in config are relative to the project folder (the one
    //holding .vmseed), so `copyFrom: config/certs` means what it says.
    pub fn config_path(&self, value: &str) -> PathBuf {
        let expanded = expand_env(value);
        let path = PathBuf::from(&expanded);
        if path.is_absolute() {
            path
        } else if let Some(rest) = expanded.strip_prefix("~/").or(expanded.strip_prefix("~\\")) {
            paths::home_dir()
                .map(|home| home.join(rest))
                .unwrap_or(path)
        } else {
            self.project_root.join(path)
        }
    }

    //The blueprint's OS in words: "rocky 9".
    pub fn os_description(&self) -> String {
        let release = self.string_or("Iso.Release", "");
        match self.blueprint.get_string("Os.Vendor") {
            Some(vendor) => format!("{vendor} {release}"),
            None => release,
        }
    }
}

//Text items from a list value; a scalar is a list of one, null is empty.
pub fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .filter(|text| !text.trim().is_empty())
            .collect(),
        Some(Value::String(text)) if !text.trim().is_empty() => vec![text.clone()],
        Some(Value::Number(n)) => vec![n.to_string()],
        _ => Vec::new(),
    }
}

fn machine_names(machines: &Mapping) -> Vec<String> {
    machines
        .keys()
        .filter_map(|key| key.as_str().map(str::to_string))
        .collect()
}

//Defaults that depend on this computer.
fn fill_machine_defaults(map: &mut Mapping) {
    if let Ok(dir) = paths::user_path("vms") {
        set_path(
            map,
            "VM.Path",
            Value::String(dir.to_string_lossy().to_string()),
        );
    }
    if let Ok(dir) = paths::user_path(files::ISO_CACHE_DIR) {
        set_path(
            map,
            "Iso.Directory",
            Value::String(dir.to_string_lossy().to_string()),
        );
    }
    set_path(map, "Guest.Timezone", Value::String(host_timezone()));
}

//This computer's IANA time zone, from /etc/localtime on Linux and macOS. UTC
//when it can't be told (always on Windows, whose zone names differ).
pub fn host_timezone() -> String {
    if let Ok(name) = std::env::var("TZ")
        && name.contains('/')
    {
        return name.trim_start_matches(':').to_string();
    }
    if let Ok(target) = std::fs::read_link("/etc/localtime") {
        let text = target.to_string_lossy().to_string();
        if let Some((_, zone)) = text.split_once("zoneinfo/") {
            return zone.to_string();
        }
    }
    if let Ok(text) = std::fs::read_to_string("/etc/timezone") {
        let zone = text.trim();
        if !zone.is_empty() {
            return zone.to_string();
        }
    }
    "UTC".to_string()
}

//Expands %NAME% (Windows style) and $NAME / ${NAME} in a path from config.
pub fn expand_env(text: &str) -> String {
    let percent = Regex::new(r"%([A-Za-z_][A-Za-z0-9_]*)%").expect("valid");
    let dollar = Regex::new(r"\$\{?([A-Za-z_][A-Za-z0-9_]*)\}?").expect("valid");
    let once = percent.replace_all(text, |caps: &regex_lite::Captures| {
        std::env::var(&caps[1]).unwrap_or_else(|_| caps[0].to_string())
    });
    dollar
        .replace_all(&once, |caps: &regex_lite::Captures| {
            std::env::var(&caps[1]).unwrap_or_else(|_| caps[0].to_string())
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(shared: &str, local: &str) -> (PathBuf, impl Drop) {
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = std::env::temp_dir().join(format!(
            "vmseed-effective-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        let folder = root.join(brand::DIR);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(files::CONFIG_FILE), shared).unwrap();
        std::fs::write(folder.join(files::LOCAL_CONFIG_FILE), local).unwrap();
        (root.clone(), Cleanup(root))
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn layers_stack_in_order() {
        let (root, _cleanup) = project(
            "blueprint: ubuntu-server\nprojectName: Acme\ncpus: 2\n",
            "cpus: 6\nusername: dev\n",
        );
        let config = Effective::load(&root, None, Mapping::new()).unwrap();
        assert_eq!(config.vm_name, "acme-ubuntu-server");
        assert_eq!(config.hostname, "acme-ubuntu-server");
        //Local beats shared.
        assert_eq!(config.number("VM.CpuCount", 0), 6);
        //Blueprint defaults fill what nobody set.
        assert_eq!(config.number("VM.DiskSizeGB", 0), 80);
        //Blueprint overrides beat the shared file.
        assert!(!config.flag("Bootstrap.InstallDesktop", true));
        //Built-in defaults underneath everything.
        assert_eq!(
            config.string("Guest.Locale").as_deref(),
            Some("en_US.UTF-8")
        );
        assert_eq!(config.string("Iso.Release").as_deref(), Some("24.04"));
        assert!(config.vm_dir().ends_with("acme-ubuntu-server"));
    }

    #[test]
    fn machines_get_their_own_names() {
        let (root, _cleanup) = project(
            "blueprint: ubuntu-server\nmachines:\n  db:\n    cpus: 8\n  web:\n    hostname: www\n",
            "",
        );
        let db = Effective::load(&root, Some("db"), Mapping::new()).unwrap();
        assert_eq!(db.vm_name, "db");
        assert_eq!(db.number("VM.CpuCount", 0), 8);
        assert_eq!(db.peers, vec!["db", "www"]);
        let web = Effective::load(&root, Some("web"), Mapping::new()).unwrap();
        assert_eq!(web.hostname, "www");
        assert!(Effective::load(&root, Some("nope"), Mapping::new()).is_err());
    }

    #[test]
    fn a_version_from_another_os_is_refused() {
        let (root, _cleanup) = project("blueprint: ubuntu-server\nosVersion: '42'\n", "");
        let error = Effective::load(&root, None, Mapping::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not look like"), "{error}");
    }

    #[test]
    fn plain_passwords_in_the_shared_file_are_flagged() {
        let (root, _cleanup) = project("blueprint: ubuntu-server\npassword: x\n", "");
        let config = Effective::load(&root, None, Mapping::new()).unwrap();
        assert_eq!(config.warnings.len(), 1);
    }

    #[test]
    fn environment_variables_expand() {
        // SAFETY: tests in this module don't read this variable concurrently.
        unsafe { std::env::set_var("VMSEED_TEST_DIR", "/x") };
        assert_eq!(expand_env("%VMSEED_TEST_DIR%/a"), "/x/a");
        assert_eq!(expand_env("${VMSEED_TEST_DIR}/b"), "/x/b");
        assert_eq!(expand_env("$NOT_SET_ANYWHERE"), "$NOT_SET_ANYWHERE");
    }
}
