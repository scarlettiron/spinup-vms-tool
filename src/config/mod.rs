//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Loading a project's config: read each file, expand the quick settings, and
//merge the layers. This follows the PowerShell tool's rules
//(Expand-ConfigAliases and Get-DevVMConfig in scripts/lib/Common.psm1).
//
//Layers, lowest to highest:
//  1. .vmynt/vm.config.yaml        shared, committed
//  2. .vmynt/vm.config.local.yaml  personal, git-ignored
//  3. the chosen Machines: entry, if any
//
//Built-in and blueprint defaults (the layers below these) arrive with `create`
//in milestone M1, together with the full validation.

pub mod effective;
pub mod user;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_norway::{Mapping, Value};

use crate::brand;
use crate::constants::config_keys::{
    ALIASES, DEFAULT_ADMINER_PORT, DEFAULT_PROJECT_NAME, SECTIONS,
};
use crate::constants::files;
use crate::utilities::text;

//A project's config after the layers are merged.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub project_root: PathBuf,
    //The merged settings, without the Machines block.
    pub settings: Mapping,
    //The keys of the Machines block, sorted. Empty when there is none.
    pub machines: Vec<String>,
    //Which machine these settings are for, if one was chosen.
    pub machine: Option<String>,
}

//The names a VM ends up with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmNames {
    pub vm_name: String,
    pub hostname: String,
}

impl ProjectConfig {
    //Loads the project's config, optionally for one entry of its Machines block.
    pub fn load(project_root: &Path, machine: Option<&str>) -> Result<Self> {
        let folder = project_root.join(brand::DIR);
        let shared_path = folder.join(files::CONFIG_FILE);
        let local_path = folder.join(files::LOCAL_CONFIG_FILE);

        let shared = expand_aliases(load_layer(&shared_path)?, &display_name(&shared_path))?;
        let mut merged = shared;
        if local_path.is_file() {
            let local = expand_aliases(load_layer(&local_path)?, &display_name(&local_path))?;
            merge_into(&mut merged, local);
        }

        //Take the Machines block out of the settings; it is a menu, not a setting.
        let machines_block = remove_key(&mut merged, "Machines");
        let mut machines: Vec<String> = Vec::new();
        let mut chosen: Option<Mapping> = None;
        if let Some(Value::Mapping(block)) = &machines_block {
            for (key, value) in block {
                let Some(name) = key.as_str() else { continue };
                machines.push(name.to_string());
                if machine.is_some_and(|wanted| wanted.eq_ignore_ascii_case(name)) {
                    let layer = match value {
                        Value::Mapping(map) => map.clone(),
                        Value::Null => Mapping::new(),
                        _ => bail!("Machines.{name} in the config must be a block of settings"),
                    };
                    chosen = Some(expand_aliases(layer, &format!("machine '{name}'"))?);
                }
            }
        }
        machines.sort();

        if let Some(wanted) = machine {
            let Some(layer) = chosen else {
                let known = if machines.is_empty() {
                    "(none defined)".to_string()
                } else {
                    machines.join(", ")
                };
                bail!("unknown machine '{wanted}'. Defined machines: {known}");
            };
            merge_into(&mut merged, layer);
            //A machine's key names its VM and hostname, unless the machine sets them.
            if get_path(&merged, "VM.Name").is_none() {
                set_path(&mut merged, "VM.Name", Value::String(wanted.to_string()));
            }
            if get_path(&merged, "Guest.Hostname").is_none() {
                set_path(
                    &mut merged,
                    "Guest.Hostname",
                    Value::String(wanted.to_string()),
                );
            }
        }

        Ok(ProjectConfig {
            project_root: project_root.to_path_buf(),
            settings: merged,
            machines,
            machine: machine.map(str::to_string),
        })
    }

    pub fn project_name(&self) -> String {
        get_string(&self.settings, "ProjectName")
            .unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_string())
    }

    pub fn blueprint(&self) -> String {
        get_string(&self.settings, "Blueprint")
            .unwrap_or_else(|| files::DEFAULT_BLUEPRINT.to_string())
    }

    //"<project>-<blueprint>" in lowercase, unless VmName or Hostname are set.
    pub fn names(&self) -> VmNames {
        let derived = format!("{}-{}", self.project_name(), self.blueprint()).to_lowercase();
        let vm_name = get_string(&self.settings, "VM.Name").unwrap_or_else(|| derived.clone());
        let hostname = get_string(&self.settings, "Guest.Hostname").unwrap_or(derived);
        VmNames { vm_name, hostname }
    }

    //The http address of the VM, published over mDNS.
    pub fn url(&self) -> String {
        format!("http://{}.local", self.names().hostname)
    }

    //Adminer's address, when the config turns it on.
    pub fn adminer_url(&self) -> Option<String> {
        let enabled = get_path(&self.settings, "Database.Adminer.Install").is_some_and(is_truthy);
        if !enabled {
            return None;
        }
        let port = get_path(&self.settings, "Database.Adminer.Port")
            .and_then(value_as_u64)
            .unwrap_or(u64::from(DEFAULT_ADMINER_PORT));
        Some(format!("http://{}.local:{port}/", self.names().hostname))
    }
}

//Reads one YAML file into a mapping. An empty file is an empty mapping.
pub fn load_layer(path: &Path) -> Result<Mapping> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Mapping::new());
    }
    let value: Value = serde_norway::from_str(&text)
        .with_context(|| format!("{} is not valid YAML", display_name(path)))?;
    match value {
        Value::Mapping(map) => Ok(map),
        Value::Null => Ok(Mapping::new()),
        _ => bail!(
            "{} must contain settings (key: value lines) at the top level",
            display_name(path)
        ),
    }
}

//Turns one layer's quick settings into the nested shape, and rejects top-level
//keys that are neither a section nor a quick setting. Section names come out in
//their canonical spelling, whatever case the file used.
pub fn expand_aliases(layer: Mapping, source: &str) -> Result<Mapping> {
    let mut sections = Mapping::new();
    let mut quick: Vec<(&'static str, Value)> = Vec::new();

    for (key, value) in layer {
        let Some(name) = key.as_str() else {
            bail!("{source}: top-level keys must be text, found {key:?}");
        };

        if let Some(section) = SECTIONS
            .iter()
            .find(|section| section.eq_ignore_ascii_case(name))
        {
            //`repos:` given a list is shorthand for Repos.Items.
            if *section == "Repos" && value.is_sequence() {
                let mut repos = Mapping::new();
                repos.insert(Value::String("Items".to_string()), value);
                sections.insert(Value::String((*section).to_string()), Value::Mapping(repos));
            } else {
                sections.insert(Value::String((*section).to_string()), value);
            }
            continue;
        }

        if let Some((_, target)) = ALIASES
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
        {
            quick.push((target, value));
            continue;
        }

        let candidates = SECTIONS
            .iter()
            .copied()
            .chain(ALIASES.iter().map(|(alias, _)| *alias));
        let suggestion = text::closest(name, candidates)
            .map(|closest| format!(" Did you mean '{}'?", lower_first(closest)))
            .unwrap_or_default();
        bail!(
            "unknown setting '{name}' in {source}.{suggestion} Every setting is listed in {} beside it.",
            files::EXAMPLE_CONFIG_FILE
        );
    }

    let mut result = sections;
    for (target, value) in quick {
        set_path(&mut result, target, value);
    }
    Ok(result)
}

//Merges `layer` over `base`. Blocks merge key by key (ignoring case); anything
//else in `layer` replaces what `base` had.
pub fn merge_into(base: &mut Mapping, layer: Mapping) {
    for (key, value) in layer {
        let existing_key = find_key(base, &key);
        match (existing_key, value) {
            (Some(existing_key), Value::Mapping(incoming)) => {
                if let Some(Value::Mapping(current)) = base.get_mut(&existing_key) {
                    merge_into(current, incoming);
                } else {
                    base.insert(existing_key, Value::Mapping(incoming));
                }
            }
            (Some(existing_key), other) => {
                base.insert(existing_key, other);
            }
            (None, other) => {
                base.insert(key, other);
            }
        }
    }
}

//Reads a dotted path like "VM.Name", ignoring case. Null and empty strings count
//as unset, as they do in the PowerShell tool.
pub fn get_path<'a>(map: &'a Mapping, path: &str) -> Option<&'a Value> {
    let mut current = map;
    let parts: Vec<&str> = path.split('.').collect();
    for (index, part) in parts.iter().enumerate() {
        let key = find_key(current, &Value::String((*part).to_string()))?;
        let value = current.get(&key)?;
        if index + 1 == parts.len() {
            return match value {
                Value::Null => None,
                Value::String(text) if text.trim().is_empty() => None,
                other => Some(other),
            };
        }
        current = value.as_mapping()?;
    }
    None
}

//Reads a dotted path as text. Numbers and booleans are turned into text.
pub fn get_string(map: &Mapping, path: &str) -> Option<String> {
    match get_path(map, path)? {
        Value::String(text) => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

//Sets a dotted path, creating blocks on the way, matching existing keys
//regardless of case.
pub fn set_path(map: &mut Mapping, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = map;
    for part in &parts[..parts.len() - 1] {
        let wanted = Value::String((*part).to_string());
        let key = find_key(current, &wanted).unwrap_or(wanted);
        let needs_block = !matches!(current.get(&key), Some(Value::Mapping(_)));
        if needs_block {
            current.insert(key.clone(), Value::Mapping(Mapping::new()));
        }
        current = current
            .get_mut(&key)
            .and_then(Value::as_mapping_mut)
            .expect("the block was just created");
    }
    let last = Value::String(parts[parts.len() - 1].to_string());
    let key = find_key(current, &last).unwrap_or(last);
    current.insert(key, value);
}

//The existing key in `map` that matches `key`, ignoring case for text keys.
pub fn find_key(map: &Mapping, key: &Value) -> Option<Value> {
    if map.contains_key(key) {
        return Some(key.clone());
    }
    let wanted = key.as_str()?;
    map.keys()
        .find(|candidate| {
            candidate
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
        })
        .cloned()
}

pub fn remove_key(map: &mut Mapping, name: &str) -> Option<Value> {
    let key = find_key(map, &Value::String(name.to_string()))?;
    map.remove(&key)
}

//YAML accepts true/false, and people also write yes/no and 1/0.
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_i64().is_some_and(|n| n != 0),
        Value::String(text) => matches!(
            text.trim().to_lowercase().as_str(),
            "true" | "yes" | "on" | "1"
        ),
        _ => false,
    }
}

pub fn value_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

//"MemoryGB" -> "memoryGB", the spelling the example files use.
fn lower_first(name: &str) -> String {
    let mut letters = name.chars();
    match letters.next() {
        Some(first) => first.to_lowercase().collect::<String>() + letters.as_str(),
        None => String::new(),
    }
}

//"vm.config.yaml" rather than a full path, for messages.
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(text: &str) -> Mapping {
        serde_norway::from_str(text).unwrap()
    }

    #[test]
    fn quick_settings_expand_to_nested_paths() {
        let expanded = expand_aliases(
            yaml("cpus: 6\nmemoryGB: 4\nrepos: [a, b]\nvm:\n  path: C:\\VMs\n"),
            "test",
        )
        .unwrap();
        assert_eq!(get_path(&expanded, "VM.CpuCount"), Some(&Value::from(6)));
        assert_eq!(
            get_path(&expanded, "vm.memorystartupgb"),
            Some(&Value::from(4))
        );
        assert_eq!(get_string(&expanded, "VM.Path").as_deref(), Some("C:\\VMs"));
        assert!(get_path(&expanded, "Repos.Items").unwrap().is_sequence());
    }

    #[test]
    fn unknown_settings_are_rejected_with_a_suggestion() {
        let error = expand_aliases(yaml("memoryGb: 4\ncpuss: 2\n"), "test")
            .unwrap_err()
            .to_string();
        assert!(error.contains("'cpuss'"), "{error}");
        assert!(error.contains("Did you mean 'cpus'"), "{error}");
    }

    #[test]
    fn later_layers_win_and_blocks_merge() {
        let mut base = expand_aliases(yaml("cpus: 2\nvm:\n  path: C:\\VMs\n"), "shared").unwrap();
        let local = expand_aliases(yaml("VM:\n  CpuCount: 8\n"), "local").unwrap();
        merge_into(&mut base, local);
        assert_eq!(get_path(&base, "VM.CpuCount"), Some(&Value::from(8)));
        assert_eq!(get_string(&base, "VM.Path").as_deref(), Some("C:\\VMs"));
    }

    #[test]
    fn empty_values_count_as_unset() {
        let map = expand_aliases(yaml("vmName: ''\nhostname: ~\n"), "test").unwrap();
        assert!(get_path(&map, "VM.Name").is_none());
        assert!(get_path(&map, "Guest.Hostname").is_none());
    }
}
