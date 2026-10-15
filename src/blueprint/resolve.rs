//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Turning a blueprint name into everything a build needs from it: the merged
//settings (following Inherits), the seed templates, and the bootstrap files.
//This is Resolve-Blueprint from scripts/lib/Common.psm1.
//
//Files are looked up along the inheritance chain, child first. A blueprint that
//lives in a project (or was downloaded) and lacks a file falls through to the
//built-in copy at the same place, so a custom blueprint only needs the files it
//changes. Bootstrap folders layer: the built-in files first, then the
//project's, so a project can replace one step without copying them all.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use include_dir::{Dir, DirEntry, File};
use serde_norway::{Mapping, Value};

use super::{EMBEDDED, Source, find, psd1};
use crate::config::{get_path, get_string, merge_into};
use crate::constants::files;
use crate::platform::Arch;

//A file a blueprint provides, built in or on disk.
#[derive(Debug, Clone)]
pub enum Asset {
    Embedded(&'static File<'static>),
    Disk(PathBuf),
}

impl Asset {
    pub fn read(&self) -> Result<Vec<u8>> {
        match self {
            Asset::Embedded(file) => Ok(file.contents().to_vec()),
            Asset::Disk(path) => {
                std::fs::read(path).with_context(|| format!("could not read {}", path.display()))
            }
        }
    }

    pub fn read_text(&self) -> Result<String> {
        let bytes = self.read()?;
        String::from_utf8(bytes).with_context(|| format!("{} is not UTF-8 text", self.describe()))
    }

    //Where the file came from, for messages.
    pub fn describe(&self) -> String {
        match self {
            Asset::Embedded(file) => format!("(built in) blueprints/{}", file.path().display()),
            Asset::Disk(path) => crate::utilities::paths::display_path(path),
        }
    }
}

//A blueprint with its inheritance applied.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub name: String,
    //The blueprint and its ancestors, child first.
    pub chain: Vec<String>,
    //The merged settings: Os, Media, Provisioning, Firmware, Packages, Defaults,
    //Overrides. The Arm64 block is already applied when the guest is arm64.
    pub data: Mapping,
    pub seed_template: Asset,
    pub meta_template: Option<Asset>,
    //Every bootstrap file, keyed by its path relative to the bootstrap folder
    //("01-base.sh", "assets/xrdp-startwm.sh"). Later folders have replaced
    //earlier ones.
    pub bootstrap: BTreeMap<String, Asset>,
}

impl Resolved {
    pub fn get(&self, path: &str) -> Option<&Value> {
        get_path(&self.data, path)
    }

    pub fn get_string(&self, path: &str) -> Option<String> {
        get_string(&self.data, path)
    }

    pub fn get_bool(&self, path: &str, default: bool) -> bool {
        self.get(path)
            .map(crate::config::is_truthy)
            .unwrap_or(default)
    }
}

//Where one link in the chain lives.
#[derive(Debug, Clone)]
enum Location {
    //A path inside the embedded blueprints folder, like "ubuntu-desktop".
    Embedded(String),
    Disk(PathBuf),
}

struct Link {
    name: String,
    location: Location,
}

pub fn resolve(name: &str, project_root: Option<&Path>, arch: Arch) -> Result<Resolved> {
    let mut chain: Vec<Link> = Vec::new();
    let mut datas: Vec<Mapping> = Vec::new();
    let mut current = Some(name.to_string());

    while let Some(link_name) = current.take() {
        if chain.iter().any(|link| link.name == link_name) {
            bail!("blueprint inheritance goes round in a circle at '{link_name}'");
        }
        let Some(found) = find(&link_name, project_root)? else {
            let available: Vec<String> = super::list(project_root)?
                .into_iter()
                .map(|blueprint| blueprint.name)
                .collect();
            bail!(
                "there is no blueprint called '{link_name}'. Available: {}",
                available.join(", ")
            );
        };
        let location = match &found.source {
            Source::Project(dir) | Source::Downloaded(dir) => Location::Disk(dir.clone()),
            Source::Embedded => Location::Embedded(link_name.clone()),
        };
        let manifest = read_manifest(&location)
            .with_context(|| format!("could not read blueprint '{link_name}'"))?;
        current = get_string(&manifest, "Inherits");
        chain.push(Link {
            name: link_name,
            location,
        });
        datas.push(manifest);
    }

    //Ancestors first, so the child wins.
    let mut data = Mapping::new();
    for layer in datas.into_iter().rev() {
        merge_into(&mut data, layer);
    }
    remove_key(&mut data, "Inherits");
    let arm = remove_key(&mut data, "Arm64");
    if arch == Arch::Aarch64 {
        match arm {
            Some(Value::Mapping(overrides)) => merge_into(&mut data, overrides),
            _ => bail!(
                "blueprint '{name}' has no Arm64 block, so it cannot install an arm64 guest. \
                 Set `arch: x86_64` on an x86 computer, or pick another blueprint."
            ),
        }
    }

    let template = |key: &str| -> Result<Option<Asset>> {
        let Some(relative) = get_string(&data, &format!("Provisioning.{key}")) else {
            return Ok(None);
        };
        match find_file(&chain, &relative) {
            Some(asset) => Ok(Some(asset)),
            None => bail!(
                "blueprint '{name}' names Provisioning.{key} '{relative}', but no such file exists"
            ),
        }
    };
    let Some(seed_template) = template("SeedTemplate")? else {
        bail!("blueprint '{name}' (or what it inherits) has no Provisioning.SeedTemplate");
    };
    let meta_template = template("MetaTemplate")?;

    let bootstrap_dirs: Vec<String> = match get_path(&data, "Provisioning.BootstrapDirs") {
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        Some(Value::String(one)) => vec![one.clone()],
        _ => match get_string(&data, "Provisioning.BootstrapDir") {
            Some(one) => vec![one],
            None => {
                bail!("blueprint '{name}' (or what it inherits) has no Provisioning.BootstrapDirs")
            }
        },
    };
    let mut bootstrap = BTreeMap::new();
    for relative in &bootstrap_dirs {
        let found = collect_folder(&chain, relative);
        if found.is_empty() {
            bail!("blueprint '{name}': the bootstrap folder '{relative}' was not found");
        }
        bootstrap.extend(found);
    }

    let first = chain.remove(0);
    let mut names = vec![first.name.clone()];
    names.extend(chain.iter().map(|link| link.name.clone()));
    Ok(Resolved {
        name: first.name,
        chain: names,
        data,
        seed_template,
        meta_template,
        bootstrap,
    })
}

fn read_manifest(location: &Location) -> Result<Mapping> {
    let text = match location {
        Location::Embedded(dir) => EMBEDDED
            .get_file(format!("{dir}/{}", files::BLUEPRINT_MANIFEST))
            .and_then(File::contents_utf8)
            .map(str::to_string)
            .with_context(|| format!("the built-in blueprint {dir} has no manifest"))?,
        Location::Disk(dir) => {
            let path = dir.join(files::BLUEPRINT_MANIFEST);
            std::fs::read_to_string(&path)
                .with_context(|| format!("could not read {}", path.display()))?
        }
    };
    psd1::parse(&text)
}

//`relative` resolved against `base` ("ubuntu-server" + "../bootstrap" is
//"bootstrap"). None when it climbs out of the blueprints folder.
fn embedded_path(base: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(base).join(relative).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

//The first link in the chain holding `relative`, child first. A blueprint on
//disk falls through to the built-in file at the same place.
fn find_file(chain: &[Link], relative: &str) -> Option<Asset> {
    for link in chain {
        match &link.location {
            Location::Embedded(dir) => {
                if let Some(file) = embedded_path(dir, relative).and_then(|p| EMBEDDED.get_file(p))
                {
                    return Some(Asset::Embedded(file));
                }
            }
            Location::Disk(dir) => {
                let path = dir.join(relative);
                if path.is_file() {
                    return Some(Asset::Disk(path));
                }
                if let Some(file) =
                    embedded_path(&link.name, relative).and_then(|p| EMBEDDED.get_file(p))
                {
                    return Some(Asset::Embedded(file));
                }
            }
        }
    }
    None
}

//Every file in the folder `relative`, taken from the first link that has it.
//For a blueprint on disk, the built-in folder at the same place goes underneath
//it, so a project's bootstrap folder only needs the steps it replaces.
fn collect_folder(chain: &[Link], relative: &str) -> BTreeMap<String, Asset> {
    for link in chain {
        let mut found = BTreeMap::new();
        let embedded_name = match &link.location {
            Location::Embedded(dir) => embedded_path(dir, relative),
            Location::Disk(_) => embedded_path(&link.name, relative),
        };
        if let Some(dir) = embedded_name.as_deref().and_then(|p| EMBEDDED.get_dir(p)) {
            collect_embedded(dir, dir.path(), &mut found);
        }
        if let Location::Disk(base) = &link.location {
            let folder = base.join(relative);
            if folder.is_dir() {
                collect_disk(&folder, &folder, &mut found);
            }
        }
        if !found.is_empty() {
            return found;
        }
    }
    BTreeMap::new()
}

fn collect_embedded(dir: &'static Dir<'static>, root: &Path, into: &mut BTreeMap<String, Asset>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(child) => collect_embedded(child, root, into),
            DirEntry::File(file) => {
                let relative = file.path().strip_prefix(root).unwrap_or(file.path());
                into.insert(slash_path(relative), Asset::Embedded(file));
            }
        }
    }
}

fn collect_disk(dir: &Path, root: &Path, into: &mut BTreeMap<String, Asset>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_disk(&path, root, into);
        } else if let Ok(relative) = path.strip_prefix(root) {
            into.insert(slash_path(relative), Asset::Disk(path.clone()));
        }
    }
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn remove_key(map: &mut Mapping, name: &str) -> Option<Value> {
    let key = map
        .keys()
        .find(|key| key.as_str().is_some_and(|k| k.eq_ignore_ascii_case(name)))
        .cloned()?;
    map.remove(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inheritance_merges_and_finds_files_up_the_chain() {
        let server = resolve("ubuntu-server", None, Arch::X86_64).unwrap();
        assert_eq!(server.chain, vec!["ubuntu-server", "ubuntu-desktop"]);
        //From the parent.
        assert_eq!(
            server.get_string("Provisioning.Method").as_deref(),
            Some("cloud-init-nocloud")
        );
        //Its own.
        assert!(!server.get_bool("Overrides.Bootstrap.InstallDesktop", true));
        assert!(server.meta_template.is_some());
        //Shared steps plus the parent's assets.
        assert!(server.bootstrap.contains_key("_runner.sh"));
        assert!(server.bootstrap.contains_key("assets/45-allow-colord.pkla"));
    }

    #[test]
    fn every_built_in_blueprint_resolves_on_both_architectures() {
        for blueprint in super::super::list(None).unwrap() {
            let resolved = resolve(&blueprint.name, None, Arch::X86_64)
                .unwrap_or_else(|e| panic!("{}: {e:#}", blueprint.name));
            assert!(resolved.bootstrap.contains_key("_vm-bootstrap.service"));
            let arm = resolve(&blueprint.name, None, Arch::Aarch64)
                .unwrap_or_else(|e| panic!("{} arm64: {e:#}", blueprint.name));
            let media = format!("{:?}", arm.get("Media"));
            assert!(
                !media.contains("x86_64") && !media.contains("amd64"),
                "{}: {media}",
                blueprint.name
            );
        }
    }

    #[test]
    fn paths_climb_out_of_a_blueprint() {
        assert_eq!(
            embedded_path("ubuntu-server", "../bootstrap").as_deref(),
            Some("bootstrap")
        );
        assert_eq!(
            embedded_path("a", "seed/x.tmpl").as_deref(),
            Some("a/seed/x.tmpl")
        );
        assert_eq!(embedded_path("a", "../../x"), None);
    }
}
