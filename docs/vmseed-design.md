# Vmseed — design

Status: **M0–M3 built; M4 (1.0) in progress** · Drafted 2026-10-13 · Replaced the PowerShell tool, removed at M4

Vmseed is the Rust rewrite of this repo's original PowerShell tool (`vm.ps1` + `scripts/`, removed
before 1.0 and still in git history). It keeps what that tool did — describe a dev VM in YAML, run one command, get a fully
provisioned desktop — and makes it:

- **cross-platform**: Windows (Hyper-V), Linux (QEMU/KVM via libvirt), macOS
  (Apple Virtualization.framework via vfkit), on x86_64 and arm64;
- **installable**: one self-contained binary from winget, Homebrew, `.deb`/`.rpm`, or a
  one-line install script — users never install Rust, PowerShell or anything else to run it;
- **project-based like ddev**: `vmseed init` in any folder, then `vmseed start`, `vmseed list`,
  `vmseed ssh` from anywhere.

All three host platforms are in scope for 1.0. The milestones in §16 only order the work.

---

## 1. Goals and non-goals

**Goals**

- Same YAML config, same blueprints, same guest bootstrap scripts as the PowerShell tool had —
  a VM built by Vmseed is the same machine that tool built.
- One binary per platform, no runtime dependencies beyond the hypervisor itself (and vfkit on
  macOS, pulled in by Homebrew).
- Every user-facing name comes from one constant (§2).
- The CLI behaves like the tools people already know: `-h`, `--help`, `help <cmd>`,
  `-v`/`--version`, shell completion, `--json`, sensible exit codes.

**Non-goals for 1.0**

- Windows guests. Every blueprint is Linux; the *host* is what becomes cross-platform.
- Downloadable blueprints. All blueprints ship inside the binary for now (they total ~350 KB);
  the lookup is designed so they can move out later (§9.4).
- A GUI.
- Remote hosts / cloud providers.

---

## 2. Naming: one place to change it

The product name is **Vmseed**; the command, package and folders are `vmseed`. The name is
expected to change, so nothing hard-codes it.

```rust
// src/brand.rs — the only place the name lives in code
pub const DISPLAY_NAME: &str = "Vmseed";                         // prose, help header, messages
pub const NAME: &str = env!("CARGO_PKG_NAME");                   // "vmseed": binary, tags, registry
pub const DIR: &str = concat!(".", env!("CARGO_PKG_NAME"));      // ".vmseed"

pub fn env_var(key: &str) -> String {                            // VMSEED_HOME, VMSEED_DEBUG, …
    format!("{}_{}", NAME.to_uppercase(), key)
}
```

- `NAME` is read from `Cargo.toml`, so the binary name, packaging (dist reads `Cargo.toml`) and
  code cannot disagree.
- Embedded templates, help text and docs use `{{app}}`, `{{App}}` and `{{dir}}` placeholders,
  filled in at runtime from `brand.rs`.
- **To rename:** change `name` in `Cargo.toml` and `DISPLAY_NAME`. That's all for the code.
- **What a constant can't rename:** the GitHub repo, the Homebrew tap formula, and the winget
  package identifier. A published winget ID is effectively permanent (renaming means a new
  package), so the name must be final **before the first public release**. Renaming during
  development is free.

---

## 3. Language, build and distribution

### 3.1 Rust

Chosen for a single static-ish binary per target, strong typing over a config format with many
optional sections, and good crates for everything below. Rust is a build-time dependency only.

Crates actually used (see `Cargo.toml`):

| Need | Crate |
|---|---|
| CLI, help, completion | `clap` (derive), `clap_complete` |
| Prompts for `init` | `inquire` |
| Config | `serde`, `serde_norway` (the maintained `serde_yaml` fork), `serde_json` |
| Embedding blueprints/templates | `include_dir` |
| Downloads + checksums | `ureq` (rustls, no OpenSSL), `sha2` |
| Password hashing (SHA-512 crypt, replaces `openssl`) | `sha-crypt` |
| Seed disk (FAT image) | `fatfs` |
| Blueprint version patterns, path variables | `regex-lite` |
| Base64 for the seed payload | `base64` |
| Colour, errors, time | `anstream`/`anstyle`, `anyhow`/`thiserror`, `humantime` |

Not crates, on purpose:

- **SSH:** Vmseed runs the system `ssh` (present on Windows 10+, macOS and every Linux), so
  users' keys, agent and `~/.ssh/config` still work.
- **Hypervisors:** each backend runs the hypervisor's own tool — `powershell.exe` (Hyper-V
  cmdlets), `virsh`, `vfkit` — so there is no C library to link and every backend compiles on
  every OS (§5.2).
- **vfkit's REST API:** a few lines of HTTP over a `std::os::unix::net::UnixStream`.
- **Tables:** a 30-line aligner in `utilities/text.rs`.

### 3.2 Release pipeline

Hand-written GitHub Actions workflows in `.github/workflows/` (no `dist`):

- `version-bump.yml` (run by hand): bumps `Cargo.toml`, moves the changelog's Unreleased notes,
  commits, tags `vX.Y.Z` and starts the release. `packaging/version.py` does the editing.
- `release.yml` (on a `v*` tag): checks the tag matches `Cargo.toml`, builds six targets on
  native runners, then `.deb` (cargo-deb), `.rpm` (cargo-generate-rpm) and `SHA256SUMS`, and
  publishes a GitHub Release. Stable versions then go to the Homebrew tap
  (`scarlettiron/homebrew-tap`), winget (`CodeScarlett.Vmseed`), Chocolatey (`vmseed`) and a
  signed apt repository served from GitHub Pages. A version with a hyphen (`-rc.1`) is a
  GitHub prerelease only.

  | Target | Runner |
  |---|---|
  | `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | windows-latest |
  | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | ubuntu-latest / ubuntu arm |
  | `aarch64-apple-darwin`, `x86_64-apple-darwin` | macOS runners |

- `ci.yml`: fmt, clippy `-D warnings`, tests on all three OSes, MSRV, shellcheck, actionlint,
  the site link check. `e2e.yml` (nightly): a real `ubuntu-server` build under libvirt on a
  Linux runner with `/dev/kvm` (§17). `pages.yml`: the website and the apt repository.
- **Requires:** release artifacts must be publicly downloadable.
- **Signing:** release binaries can be signed. Windows uses Azure Artifact Signing (formerly
  Trusted Signing) through `azure/artifact-signing-action`; macOS uses a Developer ID
  certificate plus notarization with `notarytool`. Each is optional and turns on when its
  secrets are set; unsigned Windows binaries trigger SmartScreen. Homebrew installs are not
  quarantined.
- **Provenance:** every release gets GitHub build-provenance attestations, checked with
  `gh attestation verify <file> --repo scarlettiron/vmseed`.

`docs/releasing.md` is the operational guide.

## 4. Host requirements (what `vmseed doctor` checks)

| Host | Needs | Notes |
|---|---|---|
| Windows 10/11 Pro, Enterprise, Education; Server | Hyper-V enabled (with its PowerShell module); user is admin or in **Hyper-V Administrators** | Windows **Home has no Hyper-V** — doctor says so plainly. `vmconnect.exe` (Hyper-V management tools) shows both the console and the Enhanced Session desktop. |
| Linux | `/dev/kvm`, libvirt + QEMU + OVMF, the libvirt service running, user in the `libvirt` group (not `kvm`: the system connection's QEMU opens `/dev/kvm`), libvirt's `default` network and a storage pool | `virt-viewer` for the console (optional); FreeRDP (`xfreerdp3`/`xfreerdp`) or Remmina for the desktop. |
| macOS 13+ (Apple Silicon or Intel) | `vfkit` (`brew install vfkit`; the Homebrew formula depends on it) | Desktop through Microsoft's *Windows App* (an `.rdp` file is opened); console only for a VM started with a window. |
| All | `ssh` on PATH; `ssh-keygen` recommended | `ssh-keygen` makes Vmseed's own key (§11.5). |

`doctor` prints one line per check with a fix for each failure and exits non-zero if anything
required is missing. Its last check calls the backend's `preflight()` — the same call `create`
makes before touching anything.

**Setting the computer up** (`src/host_setup.rs`). Nobody follows install steps by hand: what
can be fixed, Vmseed fixes after asking. `init` offers it at the end of a new project, `create`
when `preflight()` fails, and `doctor --fix` on demand. It lists the steps, asks (`--yes`
answers; without a terminal `create` stops and `init` only prints the hint, so installing
system software is never a side effect of a script), and prints each command as it runs.

- **Linux**: `apt-get` or `dnf` installs QEMU, libvirt and the firmware for this architecture
  (and the OpenSSH client when `ssh` is missing);
  the libvirt service is started and enabled (modular `virtqemud` sockets where enabled,
  otherwise `libvirtd`); `usermod -aG libvirt`. Each runs with `sudo`. A group added this way
  reaches only later logins, so Vmseed re-executes itself through `sg libvirt` (guarded by
  `VMSEED_REGROUPED`) — at startup too, so every later command works without a logout.
- **Windows**: `Enable-WindowsOptionalFeature` for `Microsoft-Hyper-V`, its PowerShell module and
  management clients (only those that are off), and `Add-LocalGroupMember -SID S-1-5-32-578`,
  in one `Start-Process -Verb RunAs` PowerShell, so UAC asks once. A restart (or a sign-out,
  for the group alone) is unavoidable, so the command ends there and says so.
- **macOS**: nothing; the Homebrew formula depends on vfkit.
- **Not fixable**: no `/dev/kvm`, a Windows edition without Hyper-V, a pending restart, a Linux
  without apt or dnf. These are explained instead.

The install packages (apt, Homebrew, winget, Chocolatey) run no setup of their own: most can't,
and the one that could (Chocolatey's admin install script) would change Windows and force a
restart without the person deciding to.

## 5. Architecture

### 5.1 Source layout

```
Cargo.toml                [package.metadata.brand] holds every product name (§2)
build.rs                  passes the names, git commit and target to the code
src/
  main.rs                 parse args, dispatch, map errors to exit codes
  brand.rs                §2
  errors.rs               error kinds and their exit codes (§12.3)
  platform.rs             Arch (x86_64/aarch64) and HypervisorKind, from `arch:`/`hypervisor:`
  cli/
    args.rs, help.rs      every command, option and alias; the grouped help page
    common.rs             shared by the VM commands: pick the backend, wait for a state,
                          confirm, find the guest's address
    create.rs             §12 create
    lifecycle.rs          start, stop, restart, poweroff, remove
    open.rs               open (desktop or --console)
    shell.rs              ssh, exec, logs
    status.rs             status
    snapshot.rs           snapshot create|list|restore|delete
    list.rs               list, --live, --rescan
    init.rs, config.rs, blueprints.rs, hash.rs, doctor.rs, clean.rs, completion.rs, version.rs
  config/
    mod.rs                one layer: read, expand quick settings, merge
    effective.rs          every layer + the blueprint, names checked
    user.rs               ~/.vmseed/config.yaml: the saved default identity
  plans.rs                data disks, database, files, Red Hat, hooks, repos, ssh keys
  blueprint/
    mod.rs                list, find, copy (project → downloaded → embedded, §9.2);
                          reads blueprint.yaml
    resolve.rs            inherits chain, arm64 overlay, seed templates, bootstrap tree
  seed/
    mod.rs                renders ks.cfg / user-data + /etc/vm-bootstrap.*
    disk.rs               MBR + FAT32 image, fixed-VHD footer (§10)
  media.rs                ISO lookup, download, SHA-256 check, cache
  guest/
    mod.rs                ssh wrapper, the status probe and its parser
    keys.rs               Vmseed's own key pair and known_hosts (§11.5)
  hypervisor/
    mod.rs                trait Hypervisor, VmSpec, shared helpers (run, RDP launch, tags)
    hyperv.rs             PowerShell + Hyper-V cmdlets
    libvirt.rs            virsh + generated domain XML
    vfkit.rs              detached vfkit + REST over a unix socket
    fake.rs               in-memory backend for tests (debug builds only)
  host_setup.rs           sets this computer up to run VMs, after asking (§4)
  vms.rs                  VM records and "which VM does this command mean" (§12.2)
  registry.rs             ~/.vmseed/projects.yaml (§13)
  passwd.rs               SHA-512 crypt
  constants/, utilities/  file names, config vocabulary, help groups; paths, templates,
                          names, tables, atomic writes, ui
assets/
  defaults.yaml           the built-in config layer (§8.1)
  guest/probe.sh          the status probe (§11.4)
  templates/              vm.config*.yaml, .gitignore, ssh_keys/README.md for init
blueprints/               embedded with include_dir; blueprint.yaml, seed templates and the
                          shared bootstrap steps
tests/cli.rs              runs the real binary in throwaway folders, including a full VM
                          lifecycle on the fake backend
```

A single crate; split into a workspace only if compile times demand it.

### 5.2 The hypervisor trait

As implemented in `src/hypervisor/mod.rs`:

```rust
pub trait Hypervisor {
    fn kind(&self) -> HypervisorKind;
    fn seed_format(&self) -> DiskFormat;                   // Vhd (Hyper-V) or Raw
    fn system_disk(&self) -> &'static str;                 // installer's name: "sda" or "vda"
    fn seed_file_name(&self, name: &str) -> String;        // provided: <name>-seed.vhd|.img
    fn preflight(&self) -> Result<()>;                     // doctor + create: can we talk to it?

    fn state(&self, name: &str) -> Result<VmState>;        // Running|Off|Saved|Paused|Busy|Missing
    fn info(&self, name: &str) -> Result<VmInfo>;          // cpus, memory, uptime, ip, disks, heartbeat
    fn ip(&self, name: &str) -> Result<Option<String>>;

    fn create(&self, spec: &VmSpec) -> Result<()>;         // define with ISO + seed; don't boot
    fn finish_install(&self, spec: &VmSpec) -> Result<()>; // eject, drop seed, add data disks, boot disk
    fn install_progress(&self, name: &str) -> Option<String>;

    fn start(&self, name: &str, with_console: bool) -> Result<()>;
    fn stop(&self, name: &str, mode: StopMode) -> Result<()>;   // Graceful | Force | Save; caller polls
    fn remove(&self, name: &str, vm_dir: &Path, delete_disks: bool) -> Result<()>;

    fn open_console(&self, name: &str) -> Result<()>;
    fn open_desktop(&self, name: &str, address: &str, user: &str) -> Result<()>;
    fn desktop_ready(&self, name: &str, address: Option<&str>) -> bool;

    fn snapshot_create(&self, name: &str, label: &str) -> Result<()>;
    fn snapshot_list(&self, name: &str) -> Result<Vec<Snapshot>>;
    fn snapshot_restore(&self, name: &str, label: &str) -> Result<()>;
    fn snapshot_delete(&self, name: &str, label: &str) -> Result<()>;

    fn list_tagged(&self) -> Result<Vec<Tagged>>;          // VMs carrying our tag (§13.2)
}
```

Changed from the plan:

- **Every backend compiles on every OS.** They only run other programs, so there is nothing
  platform-specific to link; the few OS calls (unix sockets, process groups, file allocation)
  are `#[cfg]`-guarded inside. This lets the XML, PowerShell and vfkit command lines be unit
  tested on any computer. `hypervisor::host()` picks the backend for this OS;
  `VMSEED_HYPERVISOR` overrides it, and `hypervisor:` in config is checked against the host.
- **`fake`** keeps its VMs in `~/.vmseed/fake-hypervisor.json`, so the real binary can be run
  command after command in tests. It is only accepted in debug builds.
- `VmSpec` is everything a backend needs (hardware, disks, firmware, switch/network/pool, the
  VM folder, ISO and seed paths, and the project path for the tag); `create` builds it from
  the effective config.
- `check_host` became `preflight()`; `detach_install_media` became `finish_install`, which also
  attaches data disks and removes the seed.

Everything above the trait — config, blueprints, seed, media, guest probing, the CLI — is shared.

## 6. Backends

### 6.1 Concern-by-concern

| Concern | Hyper-V (Windows) | libvirt (Linux) | vfkit (macOS) |
|---|---|---|---|
| Control channel | `powershell.exe -EncodedCommand`, one script per operation, JSON back | `virsh -c qemu:///system` with generated domain XML | detached `vfkit` per VM + REST on a unix socket |
| Firmware | Gen 2, Secure Boot (template from the blueprint), optional vTPM | q35 + firmware autoselection (`<os firmware='efi'>`, secure-boot feature + SMM); `virt` machine on arm64 | EFI bootloader with a per-VM variable store |
| System disk | dynamic VHDX in the VM folder | qcow2 volume in a storage pool | sparse raw file |
| Data disks | VHDX, SCSI LUN 3+ | qcow2 volumes, virtio-scsi unit 3+, `<serial>` = label | sparse raw, virtio-blk `deviceId` = label |
| Install ISO | DVD drive, LUN 1 | `cdrom`, unit 1, uploaded to the pool once | USB mass storage, read-only |
| Seed disk | fixed VHD, LUN 2 | raw volume, unit 2 | raw, virtio-blk |
| Installer's system disk | `sda` | `sda` (SCSI) | `vda` |
| Network | `Default Switch` (or `vm.switchName`) | the `default` NAT network (or `vm.network`) | Virtualization.framework NAT, fixed MAC per VM |
| Guest IP | `Get-VMNetworkAdapter` | `virsh domifaddr` (lease, agent, arp) | `/var/db/dhcpd_leases`, by MAC |
| Desktop | `vmconnect` Enhanced Session (xrdp over hv_sock) | xrdp on TCP 3389 → FreeRDP / Remmina | xrdp on TCP 3389 → `.rdp` file → Windows App |
| Console | `vmconnect.exe` | `virt-viewer --attach` (VNC on 127.0.0.1) | `vfkit --gui`, only if started with it |
| Snapshots | Hyper-V checkpoints | external disk-only snapshots, VM off | APFS clones (`cp -c`), VM off |
| Tag for discovery | VM **Notes** | domain `<metadata>` | `tag` in `vfkit.json` |
| Process lifetime | Hyper-V service | libvirtd | Vmseed spawns vfkit detached; PID + socket in the VM folder |
| `stop --save` | Save-VM | `virsh managedsave` | not supported (says so) |

The tag text is `vmseed:<machine>:<project folder>` on every backend.

### 6.2 Hyper-V notes

- One PowerShell process per operation; the script is sent `-EncodedCommand` (UTF-16LE base64),
  so nothing needs shell quoting, and values are quoted PowerShell-style (`'` doubled).
- Ported from the PowerShell tool's `HyperV.psm1` cmdlet for cmdlet: same switch check, memory rules
  (nested virtualisation forces static memory), explicit SCSI locations, boot order, integration
  services, `EnhancedSessionTransportType HvSocket`.
- Elevation: the backend never self-elevates. Permission errors, a missing module and a missing
  `vmconnect` point at `doctor --fix`, which asks and then elevates once (§4).
- Not yet run against real Hyper-V (see §17).

### 6.3 libvirt notes

- Uses the **system** connection (`qemu:///system`, override with `VMSEED_LIBVIRT_URI`) so VMs
  get the `default` NAT network and a host-reachable IP; `qemu:///session` only gets user-mode
  networking, which breaks `.local`.
- **Disks live in a storage pool** (`vm.storagePool`, default `default`, created at
  `/var/lib/libvirt/images` if missing), not in `~/.vmseed`: the system qemu runs as its own
  user and usually can't read a home folder. The seed is uploaded with `vol-upload`; the ISO is
  uploaded once and reused while its size matches.
- Domain XML is generated in code (`domain_xml`), with a UUID Vmseed keeps in the VM folder so
  the domain can be redefined after the install with the same identity.
- Disks sit on a virtio-scsi controller at Hyper-V's LUNs, so `00-disks.sh` finds data disks the
  same way on both; each disk's `<serial>` is its label as well.
- Snapshots are **external, disk-only, taken while the VM is off** (internal qcow2 snapshots
  refuse UEFI guests, whose variables live outside the disk). Reverting external snapshots
  needs libvirt ≥ 9.9.

### 6.4 vfkit notes

- No service holds VM definitions, so each VM's settings live in
  `~/.vmseed/vms/<name>/vfkit.json` (CPUs, memory, MAC, disks with device ids, ISO, display) and
  `start` launches vfkit from them, detached in its own process group, with
  `--restful-uri unix://…`. Socket paths over ~100 bytes fall back to a short `/tmp` path.
- `stop` asks via REST (`Stop` = ACPI shutdown, `HardStop` = force), falling back to the PID.
  `stop --save` is refused with a message.
- The MAC address is derived from the VM name, so the DHCP lease (and the IP) stays stable.
- Disk device ids appear in the guest as `/dev/disk/by-id/virtio-<id>`; the system disk is
  `vda`, so kickstart gets `SYSTEM_DISK=vda`.
- A console window exists only if vfkit was started with `--gui`; `open --console` on a stopped
  VM starts it with one, and explains otherwise.
- Apple Silicon runs **arm64 guests only** (§11.3). `arch:` must match the host; config
  loading refuses otherwise.
- Not yet run against a real Mac (see §17).

## 7. Opening the desktop

`vmseed open` starts the VM if needed, waits until `desktop_ready` (Hyper-V: an IP and a
healthy heartbeat; libvirt/vfkit: TCP 3389 answers), gives the display manager a few seconds,
then opens the platform's client:

| Host | Client | How |
|---|---|---|
| Windows | `vmconnect.exe` (Enhanced Session) | `vmconnect localhost <name>` |
| macOS | Microsoft *Windows App* | write `desktop.rdp` in the VM folder and `open` it |
| Linux | `xfreerdp3` → `xfreerdp` → `wlfreerdp` → `remmina` | first found on PATH, else an error naming the address |

`--no-wait` skips the wait; `--console` opens the raw console instead (§6.1).

## 8. Configuration

### 8.1 Files and layering

The schema is the one documented in `docs/configuration.md`, the PowerShell tool's schema
unchanged except for the additions in §8.3. Layers, lowest to highest:

1. Vmseed's built-in defaults
2. the blueprint's defaults
3. `.vmseed/vm.config.yaml` — shared, committed
4. `.vmseed/vm.config.local.yaml` — personal, git-ignored (username, password hash, keys, paths)
5. the selected `Machines:` entry, if any
6. command-line overrides

The flat aliases (`projectName`, `cpus`, `memoryGB`, `dbEngine`, …) and the `machines:` block
work exactly as they did in the PowerShell tool. Config files are YAML only.

As built: layer 1 is `assets/defaults.yaml`, embedded in the binary, plus three defaults that
depend on the computer (`vm.path`, `iso.directory`, the time zone). The blueprint's `Overrides`
sit between the shared and local files. `config/effective.rs` does
the merge; `plans.rs` checks every section.

Relative paths in config (`copyFrom`, `hooks.postStartFile`, `githubTokenFile`,
`redHat.activationKeyFile`, `iso.path`, …) are relative to the **project folder** — the one
holding `.vmseed/` — and may start with `~/` or use `%VAR%`/`$VAR`.

### 8.2 Validation

`vmseed config --validate` and every `create` run the same checks the PowerShell tool ran
(unknown keys with did-you-mean, ranges, blueprint/version match, database engine/version,
disk letters, files, hooks, repos, keys) through `plans::check_all`. As built, `--validate`
collects the quick checks (project name, blueprint, hostname, password) all at once, then
reports the first failure from the full checks; warnings are all shown. A missing user is a
warning for `--validate` and an error for `create`.

### 8.3 New keys

| Key | Default | Meaning |
|---|---|---|
| `hypervisor` | `auto` | backend; only the host's is accepted (`hyperv`, `libvirt`, `vfkit`) |
| `arch` | `auto` (host arch) | guest architecture, `x86_64` or `arm64`; must match the host on macOS |
| `vm.path` | `~/.vmseed/vms` | where a VM's folder lives (was `C:\VMs`). On libvirt only the build files and state are here; disks are in the pool |
| `vm.network` | `default` | libvirt network |
| `vm.storagePool` | `default` | libvirt storage pool for disks, seed and ISO |
| `iso.directory` | `~/.vmseed/isos` | download cache (was `C:\ISO`) |
| `guest.timezone` | this computer's zone, else `UTC` | read from `/etc/localtime`/`TZ`; Windows falls back to UTC |

### 8.4 `password` vs `passwordHash`

`init` only ever writes `passwordHash`. A plain `password` is still accepted for compatibility,
but `create` warns when it appears in `vm.config.yaml` (the committed file), because a password
committed there is readable by everyone with access to the repository.

---

## 9. Blueprints

### 9.1 Format

Each blueprint is a folder with a **`blueprint.yaml`**, its seed templates (`seed/`) and
optionally its own `bootstrap/` steps. Keys are camelCase like the config files (matching
ignores case): `name`, `description`, `inherits`, `os` (`family`, `vendor`, `version`,
`versionPattern`, `supportPolicy`), `media` (`isoBaseUrl`, `isoNamePattern`, `checksumFile`, or
a pinned `isoUrl` + `isoSha256`), `provisioning` (`method`, `seedLabel`, `seedFileName`,
`seedTemplate`, `metaTemplate`, `bootstrapDirs`, `bootstrapRoot`, `installSource`,
`confirmText`, `hyperVPackages`, `installTimeoutMinutes`, …), `firmware`, `packages`,
`defaults`, `overrides` and `arm64`. It is read into the same YAML value type config uses, so
blueprints and config merge with one function.

**Changed from the plan:** until M4 blueprints were PowerShell data files (`blueprint.psd1`),
because the PowerShell tool read the same files. They became YAML when that tool was removed.
A project blueprint folder that still has only a `blueprint.psd1` gets an error saying to
recreate it with `vmseed blueprints new` and move the changes across.

Per-architecture media is a top-level **`arm64`** block rather than per-arch `media:` sections:

```yaml
arm64:
  media:
    isoBaseUrl: 'https://download.rockylinux.org/pub/rocky/{Release}/isos/aarch64/'
    checksumFile: 'Rocky-*-latest-aarch64-boot.iso.CHECKSUM'
  provisioning:
    installSource: |-
      url --mirrorlist="https://mirrors.rockylinux.org/mirrorlist?arch=aarch64&repo=BaseOS-{Release}"
      repo --name="AppStream" --mirrorlist="…arch=aarch64&repo=AppStream-{Release}"
    hyperVPackages: []
```

When the guest is arm64 the block is merged over the blueprint (blocks merge key by key, lists
replace); otherwise it is dropped. A blueprint without one refuses arm64 with a message.

Seed templates keep their `{{PLACEHOLDER}}` syntax (`ks.cfg.tmpl`, `user-data.tmpl`), rendered
by plain substitution; an unreplaced token is an error.

Two `provisioning` keys are new in Vmseed:

- **`confirmText`** (Ubuntu blueprints: `'yes'`). Ubuntu's installer stops at "Continue with
  autoinstall? (yes|no)" unless `autoinstall` is on the kernel command line, which a stock ISO's
  boot menu can't have. `create` types this text plus Enter into the VM's keyboard
  (`virsh send-key`; Hyper-V's `Msvm_Keyboard`) from 75 s in — after GRUB's menu, where letters
  would edit the boot entry — every 45 s until the system disk passes 256 MB. Typed early, it
  waits in the console's input buffer. vfkit has no keyboard API, so there the install starts
  with a window and the person is told to answer.
- **`hyperVPackages`**: Hyper-V's guest daemons (`hyperv-daemons`, or Ubuntu's
  `linux-tools-virtual` / `linux-cloud-tools-virtual`), added to `packages` only when the
  hypervisor is Hyper-V. Elsewhere they only stall boot for 90 s waiting for a VMBus device.
  The `arm64` blocks set it to `[]`.

All five blueprints are embedded: `redhat-rocky-workstation` (the default),
`redhat-rhel-workstation`, `fedora-workstation`, `ubuntu-desktop`, `ubuntu-server`.

### 9.2 Resolution order

A blueprint is only ever requested **by name**. The resolver checks, first match wins:

1. `<project>/.vmseed/blueprints/<name>/` — custom or overridden, committed with the project
2. `~/.vmseed/blueprints/<name>/<version>/` — downloaded (empty in 1.0)
3. embedded in the binary

`vmseed blueprints` lists everything visible from the current folder and where each comes from.

### 9.3 Custom blueprints

```
vmseed blueprints new my-rocky --from redhat-rocky-workstation
```

copies the embedded blueprint into `.vmseed/blueprints/my-rocky/` for editing. `init` offers the
same thing (§12.4). A custom blueprint can override any file; anything it doesn't contain falls
through to the built-in file at the same place (`blueprint/resolve.rs`). Bootstrap folders
layer: the built-in steps first, then the project's, so a project replaces one step without
copying them all.

### 9.4 Later: downloadable blueprints

Moving blueprints out of the binary means filling step 2 — a `blueprints pull` command and an
index file on the releases page — and dropping blueprints from step 3. Callers don't change. The
shared bootstrap (`assets/blueprints/bootstrap/`, ~180 KB) stays embedded regardless.

---

## 10. Seed disk

The PowerShell tool created a VHDX, then ran `Mount-VHD` + `Initialize-Disk` + `New-Partition` +
`Format-Volume` — Windows-only and needing admin. Vmseed builds it in-process (`seed/disk.rs`):

1. A 256 MB file with an **MBR and one FAT32 (LBA) partition** starting at 1 MiB — the same
   shape the PowerShell tool made, rather than the unpartitioned volume first planned.
2. Formatted with `fatfs`, volume label from the blueprint (`CIDATA` for cloud-init NoCloud,
   `OEMDRV` for Anaconda), written both in the boot sector and the root directory, with a
   per-build volume id.
3. **Flat:** only the rendered seed files sit at the root (`ks.cfg`, or `user-data` +
   `meta-data`). The bootstrap tree is already embedded in them (base64 heredocs in `%post`, or
   cloud-init `write_files`); `fatfs` 0.3 writes subfolders that `fsck.vfat` complains about, and
   the installers never read them. A readable copy of everything goes to
   `<vm folder>/build/` (private permissions) for review.
4. Saved as **raw** for libvirt and vfkit, or a **fixed VHD** for Hyper-V: the raw bytes plus
   the 512-byte VHD footer (geometry and checksum per Microsoft's spec). `qemu-img info -f vpc`
   reads it back as a 256 MiB disk; `blkid` sees the label and `fsck.vfat` finds no errors.

The seed holds the password hash and any secrets in plain text, so **every backend detaches it
and Vmseed deletes the file after the install** (the PowerShell tool left it attached). The
installed system already has everything it needs.

`create --render` writes only the review copy and touches no hypervisor.

## 11. Guest side

### 11.1 Reused as-is

Everything under `blueprints/bootstrap/` and each blueprint's `bootstrap/` is embedded from
`blueprints/`, unchanged from the PowerShell tool apart from §11.2.

### 11.2 Hypervisor-aware changes

- The seed's `/etc/vm-bootstrap.env` carries `VM_HYPERVISOR=hyperv|libvirt|vfkit` and
  `VM_ARCH=x86_64|aarch64`. Unset means hyperv and x86_64.
- `20-hyperv.sh` is now **`20-guest-tools.sh`**:
  - `hyperv` — today's behaviour: Hyper-V daemons, xrdp on hv_sock (vsock).
  - `libvirt` — `qemu-guest-agent`, plus `spice-vdagent` with a desktop; xrdp on TCP 3389 with
    `security_layer=negotiate`; port opened in firewalld or ufw.
  - `vfkit` — xrdp on TCP 3389, port opened.
  - The sesman/Xorg fixes, polkit rule, `startwm.sh` and SELinux module apply to all three.
- `10-desktop.sh` adds the `hyperv_fb` kernel argument only on Hyper-V.
- `00-disks.sh` looks a data disk up by label in `/dev/disk/by-id/` (`virtio-<label>`,
  `scsi-0QEMU_QEMU_HARDDISK_<label>`) before falling back to the SCSI LUN and size.

### 11.3 arm64 guests

Required on Apple Silicon, optional on arm Linux/Windows hosts.

| Item | arm64 status | Change |
|---|---|---|
| Rocky, Fedora, Ubuntu | arm64 ISOs exist | per-arch media (§9.1) |
| RHEL | arm64 exists, behind subscription | same as x86 today |
| Google Chrome | **no Linux arm64 build** | Chromium instead (snap on Ubuntu, package elsewhere); the choice is written to `/var/lib/vm-bootstrap/browser` and `status` shows it |
| VS Code, Node, Python, Docker, ddev, Postgres, MySQL | arm64 available | check repo URLs/arch strings in each script |
| Postman | arm64 tarball | pick by `VM_ARCH` |
| mkcert, direnv, Docker/GitHub apt lines, PGDG | arm64 builds exist | `GUEST_ARCH`/`GUEST_DEB_ARCH` from `_lib.sh` |
| PostgreSQL on Fedora arm64 | PGDG has no Fedora aarch64 repo | falls back to Fedora's own package with a warning |

### 11.4 Status probe

The PowerShell tool's bash status probe is now `assets/guest/probe.sh` (plus `browser` and
`ip` lines). Vmseed sends it over `ssh -o BatchMode=yes -o ConnectTimeout=4 … 'bash -s'` with a
30-second limit and parses the same tab-separated output (`guest::Facts`). Every fact stays
optional: an unreachable guest yields a partial status, never an error.

### 11.5 Vmseed's own SSH key

Vmseed keeps a key pair in `~/.vmseed/ssh/id_ed25519` (made with `ssh-keygen` on first `create`)
and adds the public half to every guest's authorized keys, alongside keys from config and
`.vmseed/ssh_keys/`. Its own `~/.vmseed/ssh/known_hosts` is used for every connection
(`StrictHostKeyChecking=accept-new`, `HashKnownHosts=no`, and `HostKeyAlias=<hostname>`, so keys
are filed under the VM's hostname whether it was reached as `<hostname>.local` or by IP, and a
DHCP address reused by another VM never collides), and `create`/`remove` forget the host there — so `ssh`,
`exec`, `logs` and `status` work with no setup, and a rebuild never trips ssh's "REMOTE HOST
IDENTIFICATION HAS CHANGED" warning or touches `~/.ssh/known_hosts`. Without `ssh-keygen`,
`create` warns and the user's own keys are needed.

## 12. CLI

### 12.1 Commands

The real `vmseed --help`:

```
Reproducible dev VMs from a config file

Usage: vmseed <command> [name] [options]

Project:
  init        Create .vmseed/ config in this folder
  config      Show the merged config; --validate to check it

Lifecycle:
  create      Build the VM from config  (new, build)
  start       Start or resume; never provisions  (up)
  stop        Shut down, keeping the disks  (halt, down)
  restart     Stop, then start
  poweroff    Stop every VM on this computer
  remove      Delete the VM; --delete-disks removes its disks too  (rm, destroy)

Use:
  open        Open the desktop; --console for the raw console  (desktop, console)
  ssh         Open a shell in the VM
  exec        Run one command in the VM
  logs        Show bootstrap and post-start logs

Inspect:
  list        Every VM on this computer, with status and URLs  (ls)
  status      Full detail for one VM  (info, describe)

Snapshots:
  snapshot    Create, list, restore or delete snapshots

Tools:
  blueprints  List blueprints, or make your own with `blueprints new`
  hash        Make a password hash for the config
  doctor      Check this computer is ready to run VMs; --fix sets it up
  clean       Delete cached ISOs and leftover build files
  completion  Print shell completion for bash, zsh, fish, PowerShell or elvish
  version     Print the version
  help        Help for any command

Global options:
  -v, --version   Print the version [alias: -V]
      --json      Machine-readable output (list, status, config, version, blueprints...)
      --no-color  Plain output without colors (NO_COLOR is honoured too)
  -q, --quiet     Print errors only
      --verbose   More detail, and the full cause chain on errors
  -y, --yes       Answer yes to every confirmation, and never prompt
  -h, --help      Print help

Run `vmseed help <command>` or `vmseed <command> -h` for a command's options.
Docs: https://scarlettiron.github.io/vmseed/
```

Command-specific flags, as built: `create --render` (render the seed, touch no hypervisor),
`create --force` (delete and rebuild), `create --no-checkpoint`; `stop --save` / `--force`;
`remove --delete-disks`; `open --console` / `--no-wait`; `logs -f` / `-n <lines>`;
`status --follow` / `--no-ssh` / `--report`; `list --live` / `--rescan`;
`snapshot create|list|restore|delete <label> [name]`.

`--all` (every VM on this computer) belongs to `start`, `stop`, `restart`, `remove` and
`status`. On `create`, `--all` builds every machine of the project in the current folder.

`-v` means **version**, so verbose has no short flag. `vmseed` with no arguments prints the help.
Typos get "did you mean …" suggestions (clap).

Long commands (`create`) print numbered steps on stderr, so `--json` output on stdout stays
clean.

### 12.2 Choosing the VM

As implemented in `src/vms.rs`:

1. `vmseed <cmd> <name>` — matched against the current project's VM names, machine keys and
   hostnames first, then VM records (`~/.vmseed/vms/<name>/vm.json`), then every registered
   project. A name nobody knows is still passed to the hypervisor, so a VM Vmseed didn't build can
   be started or removed.
2. `vmseed <cmd>` inside a project — finds `.vmseed/` in the current folder or a parent. If the
   project defines `Machines:`, the command acts on all of them; `open`, `ssh`, `exec`, `logs`
   and `snapshot` need one VM and ask for a name.
3. `--all` — every VM with a record on this computer.

Outside a project with no name and no `--all`, the command errors and suggests `vmseed list`.

### 12.3 Exit codes and output

- `0` success · `1` failure · `2` usage error · `3` target not found.
- Colour only on a TTY; `--json` output is stable and documented, human output is not.
- Destructive commands (`remove`, `snapshot restore`, `snapshot delete`, `clean`) prompt
  unless `-y`; without a terminal and without `-y` they exit 2 instead of guessing.
- `ssh` and `exec` exit with the remote command's code; ssh's own 255 becomes a message.

### 12.4 `vmseed init`

Runs in the current folder; creates `.vmseed/` there.

```
$ cd ~/work/acme
$ vmseed init
? Project name: acme                      ← default: folder name; checked hostname-safe
? Blueprint:
  › redhat-rocky-workstation   Rocky Linux 9 + GNOME
    redhat-rhel-workstation    RHEL 9 + GNOME (needs a Red Hat account)
    fedora-workstation         Fedora + GNOME
    ubuntu-desktop             Ubuntu LTS + GNOME
    ubuntu-server              Ubuntu LTS, no desktop
    Create my own…             copy one of the above into .vmseed/blueprints to edit
? Username: scarlett                       ← skipped if a default identity exists (below)
? Full name: Scarlett Scott
? Password: ********                       ← hidden, asked twice, stored only as a hash
? Save this user as your default for future projects? (Y/n)

✔ .vmseed/vm.config.yaml                   project settings — commit this
✔ .vmseed/vm.config.local.yaml             your user and password hash — git-ignored
✔ .vmseed/vm.config.example.yaml           every option, documented
✔ .vmseed/vm.config.local.example.yaml
✔ .vmseed/ssh_keys/                        drop .pub files here
✔ .vmseed/.gitignore                       ignores the local file and private keys
✔ registered "acme" in ~/.vmseed/projects.yaml

Next: check it with `vmseed config --validate`, then build the VM with `vmseed create`.
```

- **"Create my own…"** asks for a base blueprint and a new name, then does
  `blueprints new` (§9.3) and sets `blueprint:` to it.
- **"If needed"** — the user questions are skipped when `~/.vmseed/config.yaml` holds a default
  identity (username, full name, password hash, authorized keys); `init` offers to reuse or
  replace it. They can also be skipped with `--no-user` and filled in later.
- The password is hashed immediately (SHA-512 crypt, `passwd.rs`) and never written in plain
  text.
- **Non-interactive:** `vmseed init --name acme --blueprint redhat-rocky-workstation
  --username wxs --password-hash '$6$…' --yes`. With no TTY, missing required answers are an
  error, not a prompt.
- **Re-running:** refuses if `.vmseed/` exists; `--force` rewrites the example files only and
  never touches `vm.config.local.yaml`.
- **Migrating:** `vmseed init --import <path>` copies an existing `vm.config.yaml` /
  `vm.config.local.yaml` from another project or an old PowerShell-tool checkout into `.vmseed/`.

---

## 13. Folders and the VM registry

### 13.1 Layout

```
<project>/.vmseed/                    made by `vmseed init`, committed (except the local file)
    vm.config.yaml
    vm.config.local.yaml             git-ignored
    vm.config.example.yaml
    vm.config.local.example.yaml
    ssh_keys/                        *.pub become authorized keys; a private key is the GitHub identity
    blueprints/                      only if the project has custom blueprints
    .gitignore

~/.vmseed/                            per user; VMSEED_HOME overrides
    config.yaml                      the saved default identity
    projects.yaml                    project registry (§13.2)
    isos/                            download cache (iso.directory)
    ssh/                             Vmseed's key pair and known_hosts (§11.5)
    blueprints/                      downloaded blueprints (empty in 1.0)
    tmp/                             scratch files (`clean` empties it)
    vms/<name>/
        vm.json                      the VM record (§13.2)
        build/                       rendered seed files, for review (private)
        <name>.vhdx, data disks      Hyper-V
        uuid, pool, domain.xml       libvirt (disks are in the storage pool)
        vfkit.json, disk.img, data-*.img, efi-vars.fd, vfkit.pid, vfkit.sock,
        console.log, vfkit.log, snapshots/, desktop.rdp     vfkit
```

`vm.path` moves the VM folders elsewhere; `vm.json` always stays under `~/.vmseed/vms/<name>/`
and records where the folder is. `~/.vmseed` is the same on every OS so it's always easy to
find. Disks stay out of project folders so cloud sync and backups don't pick up 25 GB files.

### 13.2 Registry and `list`

Two lists:

- `~/.vmseed/projects.yaml` maps project names to folders. `init` adds the project; `create`
  adds it too if it is missing.
- `~/.vmseed/vms/<name>/vm.json` is written by `create` and deleted by `remove`: name,
  hostname, user, project folder, machine, hypervisor, blueprint, VM folder, created time,
  whether the install finished, and the `VmSpec` it was built from.

Every VM is also **tagged in the hypervisor** (§6.1). `vmseed list --rescan` reads the tags
and recreates missing projects and records.

```
$ vmseed list
NAME                 STATUS       BLUEPRINT                 URL                              ADMINER                               PROJECT
acme-rocky           running      redhat-rocky-workstation  http://acme-rocky.local          http://acme-rocky.local:3800/         ~/work/acme
shop-ubuntu-server   stopped      ubuntu-server             http://shop-ubuntu-server.local  —                                     ~/work/shop
old                  missing      ubuntu-desktop            —                                —                                     ~/old/proj (folder gone)
```

- STATUS comes from the hypervisor: `running`, `stopped`, `saved`, `paused`, `not created`,
  or `missing`/`config error` for a project that can't be read.
- `list --live` runs the probe on every running VM in parallel threads and adds DATABASE and
  BOOTSTRAP columns.
- `--json` for scripts.

## 14. Status

`vmseed status` reproduces the PowerShell tool's status report: state and uptime, OS and kernel
(from the guest, or the blueprint from config), blueprint, address and whether `.local`
resolves, the ssh command, users, hardware, guest uptime, disks with usage, database, Adminer,
hooks, GitHub auth, browser, and bootstrap progress. The hypervisor gives host-side facts and
`probe.sh` guest-side ones. `--follow` redraws every 10 s until bootstrap completes or fails;
`--no-ssh` skips the probe; `--report` adds the guest's own step-by-step bootstrap report.

`--json` gives one object (an array for several VMs) with `name`, `hostname`, `state`, `url`,
`ip`, `user`, `bootstrap` (`complete`, `failed`, `progress` or `unknown`), `database`,
`adminer`, `github`, `browser` and the hardware fields. The nightly e2e workflow relies on it.

## 15. The PowerShell tool

Vmseed started as a port of the PowerShell tool (`vm.ps1`, `scripts/*.ps1`, `scripts/lib/*.psm1`)
that built these VMs on Windows and Hyper-V. The config schema, blueprints, seed templates and
guest bootstrap steps carried over; the PowerShell modules became `config/`, `plans.rs`,
`blueprint/`, `seed/`, `media.rs` and `hypervisor/hyperv.rs`. The tool was removed at M4 and
remains in git history. `vmseed init --import <folder>` copies `vm.config.yaml` and
`vm.config.local.yaml` from an old checkout of it.

---

## 16. Milestones

All three platforms ship in 1.0; this is the order the work was done in.

**M0 — skeleton and release pipeline. Done.**
`brand.rs`; the clap command tree; `-h`/`help`/`-v`/`version`/`completion`; config loader with
quick settings; blueprint resolver with embedded assets; `init`; `hash`; `config`; `blueprints`
/ `blueprints new`; `projects.yaml`; `doctor`; `clean`; release, CI and Pages workflows;
packaging for Homebrew, winget, Chocolatey and apt; the website.

**M1 — shared core + Windows / Hyper-V. Done (not yet run on real Hyper-V).**
Full config resolution (`config/effective.rs`, `plans.rs`, built-in defaults); the blueprint
reader (`.psd1` at the time) and blueprint inheritance; `media` (download, checksum, cache);
`seed` (rendering, FAT32 + MBR, VHD footer); `hyperv.rs`; `create`, `start`, `stop`,
`restart`, `poweroff`, `remove`, `open`, `ssh`, `exec`, `logs`, `status`, `list` with live
state, `snapshot`; VM records and target resolution; Vmseed's own SSH key; `doctor`'s hypervisor check; the fake backend and a
lifecycle integration test.

**M2 — Linux / libvirt. Done.**
`libvirt.rs` (storage pools, generated XML, virtio-scsi at Hyper-V's LUNs, external snapshots);
`20-guest-tools.sh`; xrdp on TCP; FreeRDP/Remmina launch; by-id disk lookup in `00-disks.sh`;
`.deb`/`.rpm`.

**M3 — macOS / vfkit + arm64. Done (not yet run on a real Mac).**
`vfkit.rs` (detached process, REST socket, DHCP-lease IP, APFS-clone snapshots); `arm64`
blueprint blocks with aarch64 media; Chromium fallback and other arm64 changes (§11.3); `.rdp`
launch.

**M4 — 1.0. In progress.**
Done: blueprints converted to `blueprint.yaml` (camelCase keys, `provisioning.hyperVPackages`
in place of `HyperVOnlyPackages`); the PowerShell tool (`vm.ps1`, `scripts/`) and the `.psd1`
reader removed; docs pass. Wired up but waiting on certificates and accounts: code signing
(Azure Artifact Signing on Windows, Developer ID + notarization on macOS), alongside the
build-provenance attestations every release already gets (§3.2). Still to do: real-hardware
passes on Hyper-V and on a Mac, and the first stable release with the final name.

## 17. Testing

- **Unit (in `src/`):** every embedded blueprint parses, blueprint inheritance and
  the arm64 overlay, config layering and machines, every plan's rules, seed rendering for
  kickstart and cloud-init, the FAT image read back with `fatfs`, the VHD footer checksum and
  geometry, Hyper-V scripts, libvirt XML and `virsh` output parsing, vfkit command lines, MACs,
  DHCP leases and REST states, ISO checksum files and wildcards, probe parsing, known_hosts
  editing, ssh quoting, tags.
- **CLI (`tests/cli.rs`):** the real binary in throwaway folders with `VMSEED_HOME` set,
  including a full lifecycle on the fake backend: `init` → `create --render` → `create` →
  `status --json` → `list --json` → `stop` → snapshots → `start` → `restart` → `remove`.
- **Seed disks by hand:** `blkid`, `fsck.vfat` and a loop mount for the FAT image;
  `qemu-img info -f vpc` for the VHD.
- **Linux end to end:** GitHub's Linux runners expose `/dev/kvm`, so `e2e.yml` builds
  `ubuntu-server` under libvirt nightly and waits for `status --json` to report bootstrap
  `complete`.
- **Real builds under libvirt (done):** `ubuntu-server` (with a data disk, PostgreSQL 17 and
  Adminer) and `redhat-rocky-workstation` were built on a Linux workstation with KVM. They found
  bugs the unit tests could not: password hashes with a salt glibc rejects, Ubuntu's autoinstall
  confirmation, `run_as_user` losing variables through `sudo -i`, Hyper-V daemons stalling boot
  elsewhere, the MAC changing on redefine, the clean-install snapshot taken before the install
  was finished, snapshot files left by `remove`, hashed known_hosts entries defeating
  `forget_host` (now `HostKeyAlias`), and the probe calling a finished bootstrap "in progress". All are fixed and have tests where a
  unit test can express them. Verified on the real guest: mDNS `.local` from the host, Vmseed's
  ssh key (`status`, `exec`, `logs`), the data disk found by label and mounted, all 23 bootstrap
  steps, Docker, ddev, PostgreSQL from PGDG, Python, Node via nvm, Adminer reachable from the
  host, `sudo` with the configured password, the Ubuntu confirmation typed automatically, and
  `stop` → `snapshot create` → `snapshot restore` → `start` with the change rolled back.
- **Not yet verified on real hardware:** Hyper-V and vfkit. Windows and macOS runners can't run
  nested VMs, so both get the scripted manual checklist in `docs/releasing.md` before 1.0.

## 18. Risks and open questions

| Item | Risk | Plan |
|---|---|---|
| mDNS across macOS NAT | `<host>.local` may not resolve from the host | test in M3; fall back to the VM's IP from vfkit/DHCP lease in `list`/`status` |
| FAT seed with Anaconda | label search might be fussy | resolved: MBR + one FAT32 partition, the shape the PowerShell tool used |
| Secure Boot on libvirt | distro OVMF paths differ | resolved: libvirt firmware autoselection (`firmware='efi'` + features) reads the descriptors |
| libvirt snapshots | internal snapshots refuse UEFI guests; external revert needs libvirt ≥ 9.9 | external disk-only snapshots while off; older libvirt can create but not revert |
| Hyper-V and vfkit untested on hardware | scripts and command lines are unit-tested only | manual pass before 1.0 (M4) |
| vfkit snapshots | no native support | file clones while stopped; `snapshot create` on a running VM errors clearly |
| Chrome on arm64 | not available | Chromium; `status` shows which browser was installed |
| Windows Home | no Hyper-V | `doctor` explains; out of scope for 1.0 |
| winget ID / tap name | permanent once published | finalise the name before M4 |
| Public releases | winget/Homebrew need public download URLs | decide repo visibility before M0's pre-release |
| Code signing | SmartScreen and Gatekeeper warnings | signing wired up (§3.2); needs the Azure and Apple accounts set up before 1.0 |
| Hyper-V via PowerShell | slow startup, output parsing | one script per operation, JSON only; consider WMI later |
