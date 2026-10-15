//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//File and folder names. The project folder itself (.vmynt) comes from
//brand::DIR, because it follows the product name.

//Inside a project's .vmynt/ folder.
pub const CONFIG_FILE: &str = "vm.config.yaml";
pub const LOCAL_CONFIG_FILE: &str = "vm.config.local.yaml";
pub const EXAMPLE_CONFIG_FILE: &str = "vm.config.example.yaml";
pub const LOCAL_EXAMPLE_CONFIG_FILE: &str = "vm.config.local.example.yaml";
pub const SSH_KEYS_DIR: &str = "ssh_keys";
pub const GITIGNORE_FILE: &str = ".gitignore";
pub const README_FILE: &str = "README.md";

//Shared by the project folder and the per-user folder.
pub const BLUEPRINTS_DIR: &str = "blueprints";

//Inside the per-user ~/.vmynt/ folder.
pub const USER_CONFIG_FILE: &str = "config.yaml";
pub const REGISTRY_FILE: &str = "projects.yaml";
pub const ISO_CACHE_DIR: &str = "isos";
pub const TEMP_DIR: &str = "tmp";

//Blueprints.
pub const BLUEPRINT_MANIFEST: &str = "blueprint.psd1";
//The bootstrap steps every blueprint shares. It sits beside the blueprints but is
//not one itself.
pub const SHARED_BOOTSTRAP_DIR: &str = "bootstrap";
pub const DEFAULT_BLUEPRINT: &str = "redhat-rocky-workstation";
