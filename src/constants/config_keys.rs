//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The config file vocabulary. Key matching ignores upper and lower case, so
//cpuCount, CpuCount and cpucount are the same key.

//The sections a config file may contain at the top level.
pub const SECTIONS: &[&str] = &[
    "Blueprint",
    "ProjectName",
    "VM",
    "Guest",
    "Bootstrap",
    "Database",
    "Files",
    "Hooks",
    "Git",
    "GitHub",
    "Repos",
    "Iso",
    "Machines",
    "RedHat",
    //New in Vmseed (docs/vmseed-design.md section 8.3).
    "Hypervisor",
    "Arch",
];

//Quick settings: flat names that stand for a nested setting, so
//`cpus: 8` means exactly `VM: { CpuCount: 8 }`.
pub const ALIASES: &[(&str, &str)] = &[
    ("VmName", "VM.Name"),
    ("VmPath", "VM.Path"),
    ("Cpus", "VM.CpuCount"),
    ("MemoryGB", "VM.MemoryStartupGB"),
    ("MaxMemoryGB", "VM.MemoryMaximumGB"),
    ("DiskGB", "VM.DiskSizeGB"),
    ("Switch", "VM.SwitchName"),
    ("EnhancedSession", "VM.EnhancedSession"),
    ("Hostname", "Guest.Hostname"),
    ("Username", "Guest.PrimaryUser.Username"),
    ("RealName", "Guest.PrimaryUser.RealName"),
    ("Password", "Guest.PrimaryUser.Password"),
    ("PasswordHash", "Guest.PrimaryUser.PasswordHash"),
    ("Timezone", "Guest.Timezone"),
    ("Locale", "Guest.Locale"),
    ("KeyboardLayout", "Guest.KeyboardLayout"),
    ("GitName", "Git.UserName"),
    ("GitEmail", "Git.UserEmail"),
    ("GithubUsername", "GitHub.Username"),
    ("GithubToken", "GitHub.Cli.Token"),
    ("GithubTokenFile", "GitHub.Cli.TokenFile"),
    ("SshKeyMode", "GitHub.SshKey.Mode"),
    ("ReposRoot", "Repos.Root"),
    ("OsVersion", "Iso.Release"),
    ("Release", "Iso.Release"),
    ("UbuntuRelease", "Iso.Release"),
    ("IsoPath", "Iso.Path"),
    ("RhsmActivationKey", "RedHat.ActivationKey"),
    ("RhsmOrgId", "RedHat.OrganizationId"),
    ("RhsmUsername", "RedHat.Username"),
    ("RhsmPassword", "RedHat.Password"),
    ("InstallDesktop", "Bootstrap.InstallDesktop"),
    ("InstallDocker", "Bootstrap.InstallDocker"),
    ("InstallDdev", "Bootstrap.InstallDdev"),
    ("InstallNode", "Bootstrap.InstallNode"),
    ("InstallPython", "Bootstrap.InstallPython"),
    ("InstallVSCode", "Bootstrap.InstallVSCode"),
    ("InstallChrome", "Bootstrap.InstallChrome"),
    ("InstallPostman", "Bootstrap.InstallPostman"),
    ("VSCodeExtensions", "Bootstrap.VSCodeExtensions"),
    ("ExtraVSCodeExtensions", "Bootstrap.ExtraVSCodeExtensions"),
    ("InstallRemoteAccess", "Bootstrap.InstallRemoteAccess"),
    ("NodeVersion", "Bootstrap.NodeVersion"),
    ("PythonVersion", "Bootstrap.PythonVersion"),
    ("Resolution", "Bootstrap.DisplayResolution"),
    ("ExtraPackages", "Bootstrap.ExtraAptPackages"),
    ("DotfilesRepo", "Bootstrap.DotfilesRepo"),
    ("EnableMdns", "Bootstrap.EnableMdns"),
    ("RebootWhenDone", "Bootstrap.RebootWhenDone"),
    ("DbEngine", "Database.Engine"),
    ("DbVersion", "Database.Version"),
    ("DbClient", "Database.InstallClient"),
    ("DbServer", "Database.InstallServer"),
    ("DbPort", "Database.Port"),
    ("DbListenAddress", "Database.ListenAddress"),
    ("DbAdminPassword", "Database.AdminPassword"),
    ("DbDatabases", "Database.Databases"),
    ("DbAdminer", "Database.Adminer.Install"),
    ("DbAdminerPort", "Database.Adminer.Port"),
    ("CopyFrom", "Files.Source"),
    ("CopyTo", "Files.Destination"),
    ("PostStart", "Hooks.PostStart"),
    ("PostStartFile", "Hooks.PostStartFile"),
    ("PostStartRunAs", "Hooks.RunAs"),
    ("PostStartEveryBoot", "Hooks.RunAtEveryBoot"),
];

//The project name used when a config file does not set one.
pub const DEFAULT_PROJECT_NAME: &str = "dev";

//Adminer's port when the config turns Adminer on without choosing one.
pub const DEFAULT_ADMINER_PORT: u16 = 3800;

//DNS labels, and so hostnames, stop at 63 characters.
pub const MAX_HOSTNAME_LENGTH: usize = 63;
