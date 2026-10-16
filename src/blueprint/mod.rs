//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Finding blueprints. A blueprint is one operating system and how to install it
//unattended. Code only ever asks for one by name, and this module decides where
//it comes from. First match wins:
//
//  1. <project>/.vmynt/blueprints/<name>/      custom, committed with the project
//  2. ~/.vmynt/blueprints/<name>/<version>/    downloaded (nothing downloads yet)
//  3. built into the program
//
//Splitting blueprints out of the program later means filling in step 2 and
//dropping step 3. Callers don't change. See docs/vmynt-design.md section 9.

pub mod resolve;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use include_dir::{Dir, DirEntry, include_dir};
use serde_norway::{Mapping, Value};

use crate::brand;
use crate::constants::files;
use crate::utilities::paths;

//Every blueprint, and the shared bootstrap steps, compiled into the program.
static EMBEDDED: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/blueprints");

//Where a blueprint was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Project(PathBuf),
    Downloaded(PathBuf),
    Embedded,
}

impl Source {
    pub fn label(&self) -> &'static str {
        match self {
            Source::Project(_) => "project",
            Source::Downloaded(_) => "downloaded",
            Source::Embedded => "built-in",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blueprint {
    pub name: String,
    pub description: String,
    pub source: Source,
}

//Every blueprint visible from `project_root` (or from anywhere, when None),
//sorted by name. A project or downloaded copy hides the built-in one.
pub fn list(project_root: Option<&Path>) -> Result<Vec<Blueprint>> {
    let mut found: Vec<Blueprint> = Vec::new();

    if let Some(root) = project_root {
        let folder = root.join(brand::DIR).join(files::BLUEPRINTS_DIR);
        for (name, dir) in blueprint_folders(&folder)? {
            found.push(read_folder(&name, &dir, Source::Project(dir.clone()))?);
        }
    }

    let downloads = paths::user_path(files::BLUEPRINTS_DIR)?;
    for (name, dir) in blueprint_folders(&downloads)? {
        if found.iter().any(|blueprint| blueprint.name == name) {
            continue;
        }
        if let Some(version_dir) = newest_version(&dir)? {
            found.push(read_folder(
                &name,
                &version_dir,
                Source::Downloaded(version_dir.clone()),
            )?);
        }
    }

    for dir in EMBEDDED.dirs() {
        let name = dir.path().to_string_lossy().to_string();
        if found.iter().any(|blueprint| blueprint.name == name) {
            continue;
        }
        let Some(manifest) = dir.get_file(dir.path().join(files::BLUEPRINT_MANIFEST)) else {
            //The shared bootstrap folder sits here too but is not a blueprint.
            continue;
        };
        let text = manifest.contents_utf8().unwrap_or_default();
        found.push(Blueprint {
            description: manifest_value(text, "Description").unwrap_or_default(),
            name,
            source: Source::Embedded,
        });
    }

    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

//One blueprint by name, from wherever it is found first.
pub fn find(name: &str, project_root: Option<&Path>) -> Result<Option<Blueprint>> {
    Ok(list(project_root)?
        .into_iter()
        .find(|blueprint| blueprint.name == name))
}

//Copies blueprint `base` into the project as `new_name`, ready to edit. Returns
//the new folder.
pub fn copy_to_project(base: &str, new_name: &str, project_root: &Path) -> Result<PathBuf> {
    let Some(source) = find(base, Some(project_root))? else {
        bail!(
            "there is no blueprint called '{base}'. Run `{} blueprints` to see them.",
            brand::NAME
        );
    };
    let target = project_root
        .join(brand::DIR)
        .join(files::BLUEPRINTS_DIR)
        .join(new_name);
    if target.exists() {
        bail!("{} already exists", paths::display_path(&target));
    }

    match &source.source {
        Source::Project(dir) | Source::Downloaded(dir) => copy_folder(dir, &target)?,
        Source::Embedded => {
            let dir = EMBEDDED
                .get_dir(base)
                .expect("an embedded blueprint was just listed");
            write_embedded(dir, Path::new(base), &target)?;
        }
    }

    //Rename the copy inside its manifest, so it reports its own name.
    let manifest_path = target.join(files::BLUEPRINT_MANIFEST);
    let manifest = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("could not read {}", manifest_path.display()))?;
    let renamed = set_manifest_value(&manifest, "Name", new_name);
    let renamed = set_manifest_value(
        &renamed,
        "Description",
        &format!("Custom, based on {base}: {}", source.description),
    );
    std::fs::write(&manifest_path, renamed)
        .with_context(|| format!("could not write {}", manifest_path.display()))?;
    Ok(target)
}

//Folders under `parent` named like blueprints. A missing parent is just empty.
fn blueprint_folders(parent: &Path) -> Result<Vec<(String, PathBuf)>> {
    if !parent.is_dir() {
        return Ok(Vec::new());
    }
    let mut folders = Vec::new();
    for entry in
        std::fs::read_dir(parent).with_context(|| format!("could not read {}", parent.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name != files::SHARED_BOOTSTRAP_DIR {
                folders.push((name, entry.path()));
            }
        }
    }
    Ok(folders)
}

//The highest version folder holding a manifest, for downloaded blueprints.
fn newest_version(dir: &Path) -> Result<Option<PathBuf>> {
    let mut versions: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.join(files::BLUEPRINT_MANIFEST).is_file())
        .collect();
    versions.sort();
    Ok(versions.pop())
}

fn read_folder(name: &str, dir: &Path, source: Source) -> Result<Blueprint> {
    let manifest_path = dir.join(files::BLUEPRINT_MANIFEST);
    let text = std::fs::read_to_string(&manifest_path).unwrap_or_default();
    Ok(Blueprint {
        name: name.to_string(),
        description: manifest_value(&text, "Description")
            .unwrap_or_else(|| "(no blueprint.yaml)".to_string()),
        source,
    })
}

//Reads a blueprint.yaml into the value type config uses, so blueprints and
//config merge with one function.
pub fn parse_manifest(text: &str) -> Result<Mapping> {
    match serde_norway::from_str::<Value>(text).context("blueprint.yaml is not valid YAML")? {
        Value::Mapping(map) => Ok(map),
        Value::Null => Ok(Mapping::new()),
        _ => bail!("a blueprint.yaml must be a block of settings (name:, description:, os: ...)"),
    }
}

//One top-level text value, like the description, ignoring case.
fn manifest_value(text: &str, key: &str) -> Option<String> {
    parse_manifest(text)
        .ok()
        .and_then(|map| crate::config::get_string(&map, key))
}

//Replaces the first top-level `key: ...` line, leaving every comment and the
//rest of the file as it was. Adds the line at the end when there is none.
fn set_manifest_value(text: &str, key: &str, value: &str) -> String {
    let quoted = format!("'{}'", value.replace('\'', "''"));
    let mut done = false;
    let mut output: Vec<String> = Vec::new();
    for line in text.lines() {
        if !done
            && !line.starts_with([' ', '\t', '#'])
            && let Some((left, _)) = line.split_once(':')
            && left.trim().eq_ignore_ascii_case(key)
        {
            output.push(format!("{}: {quoted}", left.trim()));
            done = true;
            continue;
        }
        output.push(line.to_string());
    }
    if !done {
        output.push(format!("{key}: {quoted}"));
    }
    output.join("\n") + "\n"
}

fn copy_folder(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("could not create {}", to.display()))?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("could not copy to {}", target.display()))?;
        }
    }
    Ok(())
}

//Writes an embedded folder to disk. Paths inside include_dir are relative to the
//embedded root, so `strip` removes the blueprint's own folder name.
fn write_embedded(dir: &Dir<'_>, strip: &Path, to: &Path) -> Result<()> {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(child) => write_embedded(child, strip, to)?,
            DirEntry::File(file) => {
                let relative = file.path().strip_prefix(strip).unwrap_or(file.path());
                let target = to.join(relative);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, file.contents())
                    .with_context(|| format!("could not write {}", target.display()))?;
                make_scripts_executable(&target)?;
            }
        }
    }
    Ok(())
}

//Shell scripts need the execute bit on Linux and macOS.
fn make_scripts_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.extension().is_some_and(|ext| ext == "sh") {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_blueprint_has_a_name_and_description() {
        let mut count = 0;
        for dir in EMBEDDED.dirs() {
            let Some(file) = dir.get_file(dir.path().join(files::BLUEPRINT_MANIFEST)) else {
                continue;
            };
            let text = file.contents_utf8().unwrap();
            let name = manifest_value(text, "Name").expect("Name");
            assert_eq!(name, dir.path().to_string_lossy(), "folder and Name agree");
            assert!(manifest_value(text, "Description").is_some());
            count += 1;
        }
        assert!(count >= 1, "at least one blueprint is embedded");
        assert!(
            EMBEDDED.get_dir(files::DEFAULT_BLUEPRINT).is_some(),
            "the default blueprint is embedded"
        );
    }

    #[test]
    fn manifest_values_can_be_replaced() {
        let text = "# name: not this\nname: 'old'\ndescription: x\nos:\n  name: inner\n";
        let changed = set_manifest_value(text, "Name", "it's new");
        assert_eq!(
            manifest_value(&changed, "name").as_deref(),
            Some("it's new")
        );
        assert!(
            changed.starts_with("# name: not this\n"),
            "comment untouched"
        );
        assert!(changed.contains("  name: inner\n"), "nested keys untouched");
        let added = set_manifest_value("description: x\n", "name", "n");
        assert_eq!(manifest_value(&added, "name").as_deref(), Some("n"));
    }
}
