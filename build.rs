//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Runs before the program is compiled. It hands the compiler a few values the
//code cannot work out on its own: the display name from Cargo.toml, the git
//commit and its date, and the platform being built for. src/brand.rs reads them
//back with env!().

use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("cargo always sets CARGO_MANIFEST_DIR");
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");

    //[package.metadata.brand] is the single place the product's names live.
    let manifest_text =
        std::fs::read_to_string(&manifest_path).expect("Cargo.toml should be readable");
    let manifest: toml::Table = manifest_text
        .parse()
        .expect("Cargo.toml should be valid TOML");
    let brand = manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("brand"))
        .and_then(|brand| brand.as_table())
        .expect("Cargo.toml needs a [package.metadata.brand] table");

    for (key, env_name) in [
        ("display-name", "APP_DISPLAY_NAME"),
        ("tagline", "APP_TAGLINE"),
        ("site-url", "APP_SITE_URL"),
    ] {
        let value = brand
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or_else(|| panic!("[package.metadata.brand] needs a `{key}` string"));
        println!("cargo:rustc-env={env_name}={value}");
    }

    //The commit and its date make `vmynt --version` precise enough for a bug report.
    //A build from a source archive has no .git folder, so both fall back to "unknown".
    let sha = git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let date = git(&["log", "-1", "--format=%cs"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=APP_GIT_SHA={sha}");
    println!("cargo:rustc-env=APP_BUILD_DATE={date}");

    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=APP_TARGET={target}");

    //Rebuild when the names change, when a commit is made, or when any embedded
    //file changes. Cargo scans a directory for changes to any file inside it.
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-changed=blueprints");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=vm.config.example.yaml");
    println!("cargo:rerun-if-changed=vm.config.local.example.yaml");
}

//Runs one git command and returns its trimmed output, or None if git is missing
//or the command fails.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
