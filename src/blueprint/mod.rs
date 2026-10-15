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

pub mod psd1;
pub mod resolve;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use include_dir::{Dir, DirEntry, include_dir};

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
            .unwrap_or_else(|| "(no blueprint.psd1)".to_string()),
        source,
    })
}

//Reads `Key = 'value'` from a blueprint.psd1, skipping the <# ... #> comment at
//the top. Blueprints move to YAML in milestone M1 (design section 9.1); until
//then this small reader is all the program needs from them.
fn manifest_value(text: &str, key: &str) -> Option<String> {
    let mut in_comment = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<#") {
            in_comment = !trimmed.contains("#>");
            continue;
        }
        if in_comment {
            in_comment = !trimmed.contains("#>");
            continue;
        }
        let Some((left, right)) = trimmed.split_once('=') else {
            continue;
        };
        if left.trim() != key {
            continue;
        }
        let value = right.trim();
        let quote = value.chars().next()?;
        if quote != '\'' && quote != '"' {
            continue;
        }
        let inner = &value[1..];
        let end = inner.find(quote)?;
        return Some(inner[..end].to_string());
    }
    None
}

//Replaces the value of the first `Key = '...'` line outside the comment block.
fn set_manifest_value(text: &str, key: &str, value: &str) -> String {
    let mut in_comment = false;
    let mut done = false;
    let mut output: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        //A line opening the comment block, or any line inside it, only moves
        //the "are we in the comment" state along.
        if in_comment || (!done && trimmed.starts_with("<#")) {
            in_comment = !trimmed.contains("#>");
        } else if !done
            && let Some((left, _)) = line.split_once('=')
            && left.trim() == key
        {
            let escaped = value.replace('\'', "''");
            output.push(format!("{left}= '{escaped}'"));
            done = true;
            continue;
        }
        output.push(line.to_string());
    }
    let mut joined = output.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
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
        let text =
            "<#\n  Name = 'not this'\n#>\n@{\n    Name        = 'old'\n    Description = 'x'\n}\n";
        let changed = set_manifest_value(text, "Name", "new");
        assert_eq!(manifest_value(&changed, "Name").as_deref(), Some("new"));
        assert!(changed.contains("Name = 'not this'"), "comment untouched");
    }
}
