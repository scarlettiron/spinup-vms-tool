# Adding or changing an OS

A blueprint is a directory under `blueprints/` holding everything OS-specific:
install media, the unattended-install template, and the provisioning scripts.
Nothing outside `blueprints/` knows which OS is being built, so a new target is a
new directory rather than a change to the tooling.

## The three cheap cases

### Switch Ubuntu release

One key. `Iso.Release` is substituted into the blueprint's `Media.IsoBaseUrl`, and
the exact point release and its checksum come from the vendor's `SHA256SUMS`:

```powershell
Iso = @{ Release = '26.04' }
```

Nothing is pinned to a filename, so this keeps working across point releases.

### Headless instead of desktop

Use the `ubuntu-server` blueprint, or just turn the desktop off:

```powershell
Blueprint   = 'ubuntu-server'
# or
Bootstrap = @{ InstallDesktop = $false; InstallVSCode = $false }
```

### A different toolchain

Add or remove a step in `blueprints/bootstrap/`. Steps are numbered shell scripts
run in order, shared by every blueprint; see the contract below.

## A new distro

Every bootstrap step is shared, so adding a distro is mostly about the installer,
not the toolchain. Three things to do:

1. **Create `blueprints/<name>/`** with a `blueprint.psd1` and a `seed/` template.
   Copy `blueprints/ubuntu-desktop` as a starting point. The keys that matter are
   below.
2. **Teach `_lib.sh` its package manager**, if it is not apt or dnf. `pkg_install`
   and friends dispatch on `$PKG_MANAGER`, which `detect_pkg_manager` sets.
3. **Add a `case` arm** to each shared step that names packages, wherever the new
   distro spells something differently. `bash -n` will not catch a missing arm, so
   grep for `PKG_MANAGER` and work through the list.

There is a second axis for when the package manager is not enough. Fedora and
Rocky both use dnf and are not the same package set, so `_lib.sh` also exports
`$OS_FAMILY`, one of `el` (RHEL and its rebuilds), `fedora`, or `debian`, derived
from `ID` and `ID_LIKE` in `/etc/os-release`. Branch on `$PKG_MANAGER` first and
reach for `$OS_FAMILY` only inside an arm where two distros sharing a package
manager genuinely differ - the comps group is `@development-tools` on Fedora and
`@development` on RHEL, for instance. Branching on `$OS_ID`, the exact distro,
means the step breaks the day someone adds AlmaLinux, so treat that as a last
resort.

**Check that the packages exist before you build.** A wrong name costs an hour of
install time to discover. Every repository publishes its index, so the list can be
checked in a couple of minutes: fetch `repodata/repomd.xml`, follow it to
`primary.xml`, and grep the `<name>` elements. That is how the Rocky blueprint
found that `vim`, `direnv` and `mkcert` do not exist there under those names, and
that `xrdp` is missing from EPEL 10 - which is why that blueprint refuses version
10 rather than failing at the end of a build.

The trade-off is deliberate. Steps live once so the distros already supported
cannot drift apart, which was a real problem when each had its own copy. The cost
is that a third distro means editing those shared steps rather than dropping in a
directory of its own. With a handful of OSes that is the better side of the trade.

If a step needs genuinely different *logic* rather than different names, a
blueprint can still override it: drop a file of the same name in
`blueprints/<name>/bootstrap/` and it wins, because later directories win when the
trees are layered.

The keys that matter in `blueprint.psd1`:

```powershell
@{
    Media = @{
        # Either resolve the current release from a published checksum file...
        IsoBaseUrl     = 'https://releases.example.org/{Release}/'
        IsoNamePattern = '*-amd64.iso'
        ChecksumFile   = 'SHA256SUMS'
        # ...or pin one exactly:
        # IsoUrl    = 'https://.../fedora-42.iso'
        # IsoSha256 = 'ABCD...'
    }
    Provisioning = @{
        Method       = 'cloud-init-nocloud'   # FAT32 disk labelled CIDATA
        SeedTemplate = 'seed/user-data.tmpl'
        MetaTemplate = 'seed/meta-data.tmpl'
        BootstrapDir = 'bootstrap'
        SourceId     = 'ubuntu-server'        # autoinstall source.id
        PowerOffAfterInstall  = $true
        InstallTimeoutMinutes = 60
    }
    Firmware = @{
        Generation         = 2
        SecureBootTemplate = 'MicrosoftUEFICertificateAuthority'
        EnableTpm          = $false           # $true for a Windows guest
    }
    Packages = @( 'openssh-server', 'curl' )  # laid down by the installer
}
```

Any distro whose installer reads cloud-init NoCloud (Ubuntu, Debian, Fedora
kickstart via a seed disk, most cloud images) works with the existing seed
mechanism - only the template changes. Something with a different unattended
format (a Windows `Autounattend.xml`, say) needs a new `Method` branch in
`scripts/lib/Seed.psm1`; the disk-building function `New-SeedDisk`
still applies, since Windows setup also reads config from an attached volume.

### Inheriting

A blueprint can name a parent and override only the difference. `ubuntu-server`
is the whole example:

```powershell
@{
    Name     = 'ubuntu-server'
    Inherits = 'ubuntu-desktop'

    # What this OS *is*. Above vm.config.yaml, so it holds.
    Overrides = @{
        VM        = @{ EnhancedSession = $false }
        Bootstrap = @{ InstallDesktop = $false; InstallVSCode = $false }
    }

    # What this OS *suggests*. Below vm.config.yaml, so the team can retune it.
    Defaults = @{
        VM = @{ CpuCount = 4; MemoryStartupGB = 8; DiskSizeGB = 80 }
    }
}
```

Templates and the bootstrap directory resolve along the inheritance chain,
child first, so a child blueprint only ships the files it actually replaces.

Choosing between the two blocks is the one judgement call when writing a
blueprint. Ask whether a shared `vm.config.yaml` setting the same key should win:

- **`Defaults`** for anything a team might legitimately retune - vCPUs, RAM,
  disk size, whether to reboot at the end.
- **`Overrides`** for anything that would make the blueprint a lie if overridden -
  a headless blueprint installing a desktop, or Enhanced Session on a machine with
  no GNOME to connect to.

Put a key in `Defaults` and a shared config that names it will silently win; put
too much in `Overrides` and the blueprint becomes rigid. Neither block can beat a
developer's own `vm.config.local.yaml`, a `Machines` block, or a command-line
parameter - an explicit human request always wins.

## The bootstrap step contract

Files in `blueprints/bootstrap/`, plus anything a blueprint adds in its own
`blueprints/<name>/bootstrap/`. The two trees are merged, later winning, so a
blueprint can add files or replace shared ones by name:

| Name | Role |
| --- | --- |
| `NN-*.sh` | a step, run in filename order |
| `_lib.sh` | helpers, sourced into every step |
| `_runner.sh` | installed as `/usr/local/sbin/vm-bootstrap` |
| `_vm-bootstrap.service` | the first-boot systemd unit |
| `assets/*` | data files, staged to `$VM_BOOTSTRAP_ROOT/assets` |

In practice the shared tree holds every step, and blueprints hold only `assets/`,
for files whose *content* differs per distro rather than their name.
`xrdp-startwm.sh` is the example: same path, same purpose, different locale file
and session mode on each OS.

Everything lands under `Provisioning.BootstrapRoot`, which is
`/usr/local/lib/vm-bootstrap` and deliberately **not** `/opt` - `/opt` is
available as a data-disk mount point, and remounting it out from under the
running bootstrap tree would fail provisioning halfway through.

Numbering leaves gaps on purpose, and the order encodes real dependencies:
disks (`00`) before anything writes to a data volume, docker (`32`) before ddev
(`33`), the SSH key (`55`) before dotfiles (`60`) and repo clones (`70`) so a
private remote can be reached.

Rules that keep steps composable:

- **Steps are sourced into a subshell**, one per step, so an `exit` ends only
  that step - use `exit 1` deliberately to mark a step failed (70-repos.sh does
  this when a clone fails, so a re-run retries it). For an early *success* exit,
  prefer `return 0 2>/dev/null || exit 0`: the `return` form also does the right
  thing if you source a step by hand in your own shell to debug it, where a bare
  `exit` would close your session.
- **Read config from `$VM_*` environment variables**, never hardcode. They come
  from `/etc/vm-bootstrap.env`, generated from `vm.config.yaml`. Adding a knob
  means adding one line to `New-BootstrapEnv` in `scripts/lib/Seed.psm1` and
  reading it in the step.
- **Gate on a flag** so a step can be turned off:
  `if ! is_true "${VM_INSTALL_DOCKER:-true}"; then ... fi`
- **Be idempotent.** Steps stamp themselves in `/var/lib/vm-bootstrap`, but
  `--force` re-runs them and people re-run steps by hand.
- **Root runs the step**; use `run_as_user` for anything that belongs to the
  developer's account (nvm, pipx, dotfiles).

Helpers available from `_lib.sh`: `log`, `warn`, `die`, `have_cmd`, `is_true`,
`pkg_install`, `pkg_install_with_recommends`, `pkg_upgrade_all`, `pkg_cleanup`,
`run_as_user`, `ensure_line`, `add_apt_repo`, `add_dnf_repo`, `admin_group`,
`firewall_allow_service`, `selinux_restore`.

## Verifying a change without building

```powershell
.\scripts\New-DevVM.ps1 -RenderOnly -Blueprint my-new-os
```

Renders the seed into `build\<vm>\` and touches no Hyper-V state. Read the
generated `user-data`, confirm your step was embedded and your variables are
set in `vm-bootstrap.env`, then build for real.
