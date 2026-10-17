# Changelog

All notable changes to Vmseed are recorded here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0, a minor
version bump (0.1 → 0.2) may include breaking changes to the CLI or config format.

## [Unreleased]

### Added

- **Computer setup**: `vmseed init` ends by offering to set the computer up when it can't
  run VMs yet, `create` offers the same when it can't reach the hypervisor, and
  `vmseed doctor --fix` does it on demand. On Linux it installs QEMU, libvirt and the UEFI
  firmware with apt or dnf, starts libvirt, and adds you to the `libvirt` group, with no need
  to log out. On Windows it turns on Hyper-V and adds you to Hyper-V Administrators, with one
  administrator prompt and then a restart. It always lists the steps and asks first; without a
  terminal it only says what to run.
- **CI tests that setup** on clean Ubuntu 24.04, Debian 13 and Fedora machines.

- **Versioning**: `packaging/version.py` reads and bumps the version in `Cargo.toml`
  (the single source of truth), checks release tags against it, and moves the
  `## [Unreleased]` notes in this file under each new version.
- **One-click releases**: the *Version bump* workflow bumps the version, updates the
  changelog and `Cargo.lock`, commits, tags and starts the release. Prereleases
  (`-alpha.N`, `-beta.N`, `-rc.N`) and hotfix branches are supported.
- **Nightly end-to-end test** that builds a real VM under libvirt/KVM on a GitHub runner
  and exercises `create`, `status`, `exec`, `snapshot`, `stop` and `remove`.
- **CI checks** for the minimum supported Rust version, workflow files (actionlint),
  guest shell scripts (shellcheck), the changelog, and the website's links and SEO tags.
- **Dependabot** for Rust crates and GitHub Actions, grouped weekly.
- **Code signing**, optional and switched on by secrets: Windows binaries through Azure
  Artifact Signing, macOS binaries with a Developer ID certificate and notarization.
- **Build provenance**: every release file gets a GitHub attestation, checked with
  `gh attestation verify <file> --repo scarlettiron/vmseed`.
- **winget's first version is detected**: the release skips winget with a warning until
  the package exists in winget-pkgs, instead of failing.
- **[docs/publishing-guide.md](docs/publishing-guide.md)**, a click-by-click walkthrough
  of the store accounts, secrets and the first release.
- **Building and running VMs** on all three backends:
  - `create` (`new`, `build`): checks the config, downloads and verifies the install ISO,
    writes the seed disk, installs unattended, takes a `clean-install` snapshot and boots the
    VM to provision itself. `--render` previews the seed files, `--force` rebuilds,
    `--no-checkpoint` skips the snapshot.
  - `start`, `stop` (`--save`, `--force`), `restart`, `poweroff`, `remove` (`--delete-disks`).
  - `open` (desktop, or `--console`; `--no-wait`), `ssh`, `exec`, `logs` (`-f`, `-n`).
  - `status` with guest facts over ssh, `--follow`, `--no-ssh`, `--report` and `--json`.
  - `snapshot create|list|restore|delete`.
- **Hyper-V backend** (Windows): PowerShell Hyper-V cmdlets, ported from the PowerShell tool,
  with Enhanced Session through `vmconnect`.
- **libvirt/KVM backend** (Linux): `qemu:///system`, disks in a libvirt storage pool, generated
  domain XML with UEFI firmware autoselection and Secure Boot, virtio-scsi disks, external
  snapshots, `virt-viewer` console, FreeRDP or Remmina desktop.
- **Apple Virtualization backend** (macOS) through vfkit: a detached vfkit per VM controlled over
  its REST socket, APFS-clone snapshots, the desktop through Microsoft's Windows App.
- **arm64 guests**: every blueprint has an `arm64` block with aarch64 installers; Chromium
  replaces Chrome on arm64.
- **Install media**: the current point release is read from the vendor's checksum file, the ISO
  is downloaded once to `~/.vmseed/isos` and verified with SHA-256.
- **Seed disk built in-process**: an MBR + FAT32 image (`CIDATA` or `OEMDRV`), and a fixed VHD
  for Hyper-V. No admin rights or Windows disk tools needed.
- **Vmseed's own SSH key** in `~/.vmseed/ssh/`, added to every VM, with its own `known_hosts`, so
  `ssh`, `exec`, `logs` and `status` work with no setup and rebuilds never trip host-key
  warnings.
- **`list`** shows live state from the hypervisor; `--live` adds database and bootstrap
  columns, probed in parallel; `--rescan` rebuilds the project list and VM records from the
  tags each hypervisor keeps.
- **`doctor`** ends with a hypervisor check that connects the same way `create` does.
- **`config --validate`** runs every check `create` runs.
- **New config keys**: `hypervisor`, `arch`, `vm.network`, `vm.storagePool`; `vm.path` now
  defaults to `~/.vmseed/vms`, `iso.directory` to `~/.vmseed/isos`, and the time zone to the
  computer's.
- **A fake hypervisor** (debug builds only) and an integration test of a VM's whole lifecycle.

### Changed

- Setup hints in `doctor` and in hypervisor errors now point at `vmseed doctor --fix`
  instead of listing commands. The commands are in
  [docs/troubleshooting.md](docs/troubleshooting.md#setting-up-by-hand).
- `doctor` on Linux no longer asks for the `kvm` group: libvirt's own QEMU opens `/dev/kvm`.
- **Renamed from Vmynt to Vmseed.** The command is `vmseed`, the folders are `.vmseed/` and
  `~/.vmseed/`, and the environment variables start with `VMSEED_`. No version under the
  old name was ever published, so there is nothing to migrate.
- The release workflow can be started by hand for an existing tag, builds the tagged
  commit in every job, and takes its release notes from this file through
  `packaging/version.py`.
- The guest step `20-hyperv.sh` is now `20-guest-tools.sh`, and sets up the right guest
  tools and remote desktop for Hyper-V, libvirt or vfkit (`VM_HYPERVISOR`, `VM_ARCH`).
- The seed disk is detached and deleted once the OS is installed, on every backend: it held
  the password hash and any secrets. The PowerShell tool left it attached.
- Relative paths in config are relative to the project folder (the one holding `.vmseed/`).
- The example configs use neutral example names.
- **Blueprints are `blueprint.yaml`** with camelCase keys (`os`, `media`, `provisioning`,
  `arm64`, …), like the config files, instead of PowerShell data files (`blueprint.psd1`). A
  project blueprint that still has only a `blueprint.psd1` is refused with a message: recreate
  it with `vmseed blueprints new` and move your changes across.
- Hyper-V's guest tools are listed in `provisioning.hyperVPackages`, which is added to
  `packages` only on Hyper-V. It replaces `Provisioning.HyperVOnlyPackages`, and the `arm64`
  blocks no longer repeat the whole package list.

### Removed

- The "not available yet" stubs for VM commands.
- **The PowerShell tool**: `vm.ps1`, `scripts/`, `docs/powershell-tool.md` and the root
  `ssh_keys/` folder. Vmseed replaces it; it remains in the git history.
  `vmseed init --import <folder>` still copies config files from an old checkout of it.
- The reader for `.psd1` blueprints.

### Fixed

Found by building real VMs under libvirt/KVM (ubuntu-server and redhat-rocky-workstation):

- **Passwords hashed by `vmseed hash` and `vmseed init` were refused at login.** The hash
  carried a 22-character salt; SHA-512 crypt uses 16, and glibc compares the full string.
  Hashes are now written as `$6$<16-character salt>$<hash>`, the way `openssl passwd -6`
  writes them, and older hashes in a config are converted when the seed is built.
- **Ubuntu installs stopped at "Continue with autoinstall? (yes|no)".** Blueprints can now
  set `provisioning.confirmText`, which Vmseed types into the VM (libvirt and Hyper-V); on
  macOS the install window opens so you can answer it.
- **Bootstrap steps run as the user lost their variables** (`30-node.sh` failed to load nvm
  on Ubuntu 24.04): `run_as_user` no longer goes through `sudo -i`, whose extra shell
  expanded `$VARS` before the command could set them.
- **Hyper-V's guest daemons delayed every boot by 90 seconds on other hypervisors.**
  Blueprints list them in `provisioning.hyperVPackages`, installed only on Hyper-V.
- **libvirt VMs got a new MAC address after the install.** Each VM now keeps a MAC derived
  from its name.
- **The clean-install snapshot was taken while the installer's ISO and seed were attached.**
  On libvirt that left the VM writing beneath its own snapshot, and restoring it would have
  re-attached a deleted seed. `create` now finishes the install first and snapshots the final
  machine before its first boot, on every backend.
- **`remove --delete-disks` left snapshot files behind on libvirt.** It now deletes exactly the
  files the VM and its snapshots use (never a shared ISO, and never another VM's volumes by a
  name pattern).
- **A rebuilt VM was refused over ssh ("REMOTE HOST IDENTIFICATION HAS CHANGED")** where the
  system hashes known_hosts entries. Vmseed's known_hosts now files keys under the VM's
  hostname (`HostKeyAlias`), unhashed, so rebuilds and reused DHCP addresses are handled.
- **`status` reported bootstrap "in progress" forever after it finished** (also in the
  PowerShell tool): the probe now trusts the `complete` marker before systemd's state.
- **`logs` exited 1 when no post-start hooks were configured.**

## [0.1.0] - 2026-10-13

The first pre-release of Vmseed, the cross-platform Rust rewrite of the PowerShell dev VM tool.
It lays the foundation: the CLI, packaging and the website. It doesn't build VMs yet. That
arrives with the Hyper-V backend in the next milestone (see
[docs/vmseed-design.md §16](docs/vmseed-design.md#16-milestones)).

### Added

- **Rust CLI scaffold** with the full command tree, grouped help (`vmseed`, `vmseed help`,
  `-h`/`--help`), version output (`-v`/`-V`/`--version`/`vmseed version`), "did you mean"
  suggestions for typos, and global `--verbose`, `--quiet`, `--json`, `--no-color` and `--yes`
  flags.
- **Working commands:**
  - `init` — creates `.vmseed/` in the current folder, asking for the project name, the blueprint
    (or making your own from an existing one), and your username and password, stored only as a
    SHA-512 crypt hash
  - `config` — show the merged config
  - `blueprints` — list the built-in and project blueprints; `blueprints new` makes your own
  - `list` — list the projects registered on this machine
  - `hash` — make a SHA-512 crypt password hash, with no `openssl` needed
  - `doctor` — check that this machine is ready to run VMs
  - `clean` — delete cached downloads
  - `completion` — shell completion for bash, zsh, fish and PowerShell
- **Lifecycle commands** (`create`, `start`, `stop`, `open`, `ssh`, `status`, `remove` and the
  rest) are present in the command tree and help, and report that they're planned.
- **Single source for the product name**: `Cargo.toml` and `src/brand.rs`, so the name can be
  changed in one place.
- **Release pipeline**: a tagged release builds for Windows, macOS and Linux on x86_64 and arm64,
  publishes archives with a `SHA256SUMS` file and `.deb` packages, and publishes to winget,
  Chocolatey, Homebrew (`scarlettiron/tap`) and a signed apt repository.
- **Website** on GitHub Pages, built from portable static components in `site/`.
- **Open-source project files**: MIT license, contributing guide, security policy, code of
  conduct, issue and pull request templates.

### Changed

- The README now describes Vmseed. The original PowerShell tool's README moved to
  `docs/powershell-tool.md`. The PowerShell tool itself is unchanged
  and keeps working until Vmseed reaches parity on Windows.

[Unreleased]: https://github.com/scarlettiron/vmseed/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/scarlettiron/vmseed/releases/tag/v0.1.0
