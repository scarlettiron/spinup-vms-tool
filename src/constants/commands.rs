//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//How the top-level help groups the commands. Every command must appear in
//exactly one group; a test in src/cli/help.rs checks that.
pub const COMMAND_GROUPS: &[(&str, &[&str])] = &[
    ("Project", &["init", "config"]),
    (
        "Lifecycle",
        &["create", "start", "stop", "restart", "poweroff", "remove"],
    ),
    ("Use", &["open", "ssh", "exec", "logs"]),
    ("Inspect", &["list", "status"]),
    ("Snapshots", &["snapshot"]),
    (
        "Tools",
        &[
            "blueprints",
            "hash",
            "doctor",
            "clean",
            "completion",
            "version",
            "help",
        ],
    ),
];
