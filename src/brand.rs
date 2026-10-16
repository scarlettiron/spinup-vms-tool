//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The product's names, in one place. Nothing else in the code spells them out.
//
//Every value here comes from Cargo.toml: the package name, and the
//[package.metadata.brand] table (passed in by build.rs). To rename the product,
//change Cargo.toml. See docs/releasing.md for what cannot be renamed once
//published.

//Shown in prose and the help header, for example "Vmynt".
pub const DISPLAY_NAME: &str = env!("APP_DISPLAY_NAME");

//The command people type, and the prefix for files, tags and environment
//variables, for example "vmynt".
pub const NAME: &str = env!("CARGO_PKG_NAME");

//The folder made in a project by `init`, and the per-user folder in the home
//directory, for example ".vmynt".
pub const DIR: &str = concat!(".", env!("CARGO_PKG_NAME"));

//One line saying what the tool does.
pub const TAGLINE: &str = env!("APP_TAGLINE");

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
pub const SITE_URL: &str = env!("APP_SITE_URL");

//What `--version` prints after the name: "0.1.0 (a1b2c3d 2026-10-13, x86_64-pc-windows-msvc)".
pub const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("APP_GIT_SHA"),
    " ",
    env!("APP_BUILD_DATE"),
    ", ",
    env!("APP_TARGET"),
    ")"
);

pub const GIT_SHA: &str = env!("APP_GIT_SHA");
pub const BUILD_DATE: &str = env!("APP_BUILD_DATE");
pub const TARGET: &str = env!("APP_TARGET");

//Builds an environment variable name with the product prefix, so
//env_var("HOME") is "VMYNT_HOME".
pub fn env_var(key: &str) -> String {
    format!("{}_{}", NAME.to_uppercase(), key)
}

//Fills the {{app}}, {{App}}, {{dir}}, {{site}} and {{repo}} placeholders used by
//the embedded templates and help text.
pub fn placeholders() -> Vec<(&'static str, String)> {
    vec![
        ("app", NAME.to_string()),
        ("App", DISPLAY_NAME.to_string()),
        ("dir", DIR.to_string()),
        ("site", SITE_URL.to_string()),
        ("repo", REPOSITORY.to_string()),
    ]
}
