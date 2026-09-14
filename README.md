# dev-vm

Reproducible Linux desktop VMs on Hyper-V, defined entirely in config. Fedora by
default, Rocky Linux or Red Hat Enterprise Linux when you need to match a Red Hat
server, Ubuntu on request. One blueprint per kind of machine.

Clone the repo, set your username, run one script. You get a full GNOME desktop
with the team's toolchain, built the same way on every laptop.

```powershell
git clone <this-repo> dev-vm
cd dev-vm

# your settings - YAML or PowerShell, whichever you prefer
Copy-Item vm.config.local.example.yaml vm.config.local.yaml

# optional: drop in the SSH key you already use for GitHub
Copy-Item $HOME\.ssh\id_ed25519* .\ssh_keys\

# then, from an ELEVATED PowerShell:
.\vm.ps1 create
```

Roughly 30-40 minutes later you have a machine you can log into, with git,
nvm + Node, Python, Docker, ddev, VS Code, Chrome, a shared clipboard, and a resizable
desktop, on a three-disk layout, with GitHub access already set up.

### The commands

`vm.ps1` at the root is the front door for everything. Run it with no arguments to
get this list back.

| Command | What it does | Runs | Also |
| --- | --- | --- | --- |
| `create` | build a VM from config | `New-DevVM.ps1` | `new`, `build` |
| `render` | show what a build would do, without touching Hyper-V | `New-DevVM.ps1 -RenderOnly` | |
| `open` | start if needed, then open the desktop | `Open-DevVM.ps1` | `desktop`, `console` |
| `start` | start or resume; never provisions | `Start-DevVM.ps1` | `up` |
| `stop` | shut down and free the RAM; keeps the disks | `Stop-DevVM.ps1` | `halt`, `down` |
| `status` | what each machine is, and how it is doing | `Get-DevVMStatus.ps1` | `info` |
| `remove` | delete the VM; needs `-DeleteDisks` to touch the disks | `Remove-DevVM.ps1` | `destroy`, `rm` |
| `hash` | make a SHA-512 crypt hash for `PasswordHash` | `New-PasswordHash.ps1` | |

```powershell
.\vm.ps1 create -Machine ubuntu-gui    # build the Ubuntu desktop VM
.\vm.ps1 open   -Machine ubuntu-gui    # start it if needed, open the desktop
.\vm.ps1 stop                          # shut down, free the RAM, keep everything
.\vm.ps1 status -Report                # including the guest's own bootstrap report
.\vm.ps1 render                        # dry run, needs no elevation
```

Everything after the command is forwarded verbatim, so any flag the underlying
script takes works here, and the exit code comes back so a failure still fails in a
shell chain. `vm.ps1` holds no defaults and validates nothing of its own, so calling
the scripts under `scripts/` directly behaves identically.

Most commands need an elevated PowerShell, because the Hyper-V cmdlets do.
`render` and `hash` do not.

---

## What actually happens

```
.\vm.ps1 create
        |
        |-- merge config      vm.config.yaml + vm.config.local.yaml + blueprint
        |-- resolve media     read the vendor's checksum file, download, verify
        |-- build seed        render the unattended install onto a labelled VHDX
        |-- create VM         gen 2, Secure Boot, dynamic memory, NAT switch
        |                     data disks stay unattached, so the installer
        |                     cannot pick one as its target
        |-- boot installer    kickstart or autoinstall, then powers off
        |-- checkpoint        "clean-install (pre-bootstrap)"  roll back point
        |-- eject ISO         attach the data disks, boot from disk, start
        |
        v
   first boot: vm-bootstrap.service
        |-- 00-disks       format, label and mount drives E: and L:
        |-- 01-base        upgrade, build tools, CLI kit
        |-- 05-users       any additional accounts
        |-- 06-files       optional files copied in from this laptop
        |-- 10-desktop     GNOME, Xorg, framebuffer resolution
        |-- 15-mdns        avahi, reachable as <hostname>.local
        |-- 20-hyperv      integration services + Enhanced Session (xrdp/vsock)
        |-- 30-node        nvm + node + corepack (yarn/pnpm)
        |-- 31-python      python, pipx, uv, ruff
        |-- 32-docker      docker engine + compose v2 + buildx
        |-- 33-ddev        ddev + mkcert local CA
        |-- 35-database    optional postgres or mysql, client and/or server
        |-- 36-adminer     optional adminer, served by php on its own port
        |-- 40-vscode      vs code
        |-- 41-chrome      google chrome, from Google's repo
        |-- 44-remote-access  vpn, rdp and ssh tunnelling clients
        |-- 52-git         git identity and defaults
        |-- 55-github      ssh key, known_hosts, gh cli
        |-- 60-dotfiles    optional dotfiles repo
        |-- 70-repos       clone the configured repositories
        |-- 80-hooks       optional post-start hooks from config
        |-- 99-finalize    summary + reboot into GDM
```

Three deliberate choices are worth knowing about:

**The unattended config arrives on a small labelled FAT32 disk.** Both installers
support this and nothing else is needed: cloud-init's NoCloud datasource reads
`user-data` from a volume labelled `CIDATA`, and Anaconda reads `ks.cfg` from one
labelled `OEMDRV`. No ISO repacking, no HTTP server, no keystrokes at the boot
menu, which is what makes "clone and run" true on a stock Windows laptop.

**Neither installer uses the desktop ISO.** Fedora installs from the Everything
netinst image with kickstart selecting the Workstation environment; Ubuntu
installs from the Server ISO and adds GNOME on first boot. Both are the paths the
vendors build for automation, both keep the package set explicit and diffable
rather than implicit in an image, and the netinst keeps the download small.

**Data disks are attached after the install, never during it.** An installer
picks its target from what it can see, and `size: largest` or `clearpart --all`
against a 50 GB data disk is a bad afternoon. A disk that is not plugged in cannot
be chosen or erased.

### Watching it run

Both halves announce each step as they reach it and print a report at the end, so
a build that goes wrong tells you where without needing a log dug out first.

The host script numbers its steps and times them:

```
==> [4/8] Building the seed disk
    rendered kickstart-oemdrv seed: ...\build\acme-fedora-workstation\seed\ks.cfg
    ok (3s)
```

and closes with a table. Host steps depend on each other, so a failure stops the
build rather than carrying on, and the report says which step stopped it and how
many never ran:

```
  #   STEP                                     STATUS     TIME
  --- ---------------------------------------- ---------- -----
  3   Resolving install media                  ok         41s
  4   Building the seed disk                   FAILED     2s
  5-8 4 remaining step(s)                      not reached

  4 step(s) attempted: 3 ok, 1 failed, 4 not reached   total 58s

  FAILED at: Building the seed disk
```

The guest bootstrap does the opposite: a failed step is reported and skipped past,
because one missing tool should not strand the whole machine. Its report is
written to a file in the VM as well as printed, and is described under
[day-to-day](#day-to-day) below.

---

## Configuration

**Two layers of YAML.** `vm.config.yaml` is the team standard and is committed.
Your own overrides go in `vm.config.local.yaml`, which is git-ignored and only
needs the handful of keys you want to differ:

```powershell
Copy-Item vm.config.local.example.yaml vm.config.local.yaml
```

**The settings people actually change sit in one flat block at the top**, as
shortcuts for the nested settings below - `cpus` *is* `VM.CpuCount`:

```yaml
projectName: scott
username:    scarlett
cpus:        8
memoryGB:    16
gitEmail:    scott@example.com
repos:
  - git@github.com:acme/web.git
```

Names are case-insensitive, so YAML stays lower-case and `.psd1` stays
PascalCase. Anything without a shortcut is written out in full, and both styles
can appear in the same file. A misspelled setting **fails the build** with the
list of valid names rather than being silently ignored.

Full list of shortcuts, the YAML subset, and every setting:
[docs/configuration.md](docs/configuration.md).

### Layers

Each overrides the one before. Set a key in whichever layer it belongs to and
the rest is inherited:

| Layer | Source | Purpose |
| --- | --- | --- |
| 1 | `blueprint.Defaults` | what this OS *suggests* - sizing, nice-to-haves |
| 2 | `vm.config.yaml` | the team standard, committed |
| 3 | `blueprint.Overrides` | what this OS *is* - a server blueprint has no desktop |
| 4 | `vm.config.local.yaml` | per-developer, git-ignored |
| 5 | `Machines.<name>` | the per-machine block, when `-Machine` is used |
| 6 | `-Parameter` | one-off overrides on the command line |

Layers 1 and 3 both come from the selected blueprint, and sitting on opposite
sides of the team standard is the point: the team decides how big a VM is, but
choosing `ubuntu-server` has to actually make it headless rather than being
quietly undone by an `InstallDesktop = $true` in the shared config. A human
naming a key in their own local config or on the command line still wins over
both.

A local override only names what it changes - `vm.config.local.yaml`:

```yaml
projectName: scott
username:    scarlett
realName:    Scott Scarlett
memoryGB:    16
```

or the same thing as `vm.config.local.yaml`:

```powershell
@{
    ProjectName = 'scott'
    Username    = 'scarlett'
    RealName    = 'Scott Scarlett'
    MemoryGB    = 16
}
```

`projectName` sets the VM name and the hostname together, to
`scott-fedora-workstation` here, which is the simplest way to avoid colliding
with a teammate on the same switch. Set `vmName` and `hostname` directly if you
want something that is not `<project>-<blueprint>`.

See what a config produces before creating anything - no elevation needed:

```powershell
.\scripts\New-DevVM.ps1 -RenderOnly
```

That writes the fully rendered cloud-init and generated bootstrap files to
`build\<vm-name>\`, so a config change can be reviewed as a diff.

### Users

`Guest.PrimaryUser` is the account the installer creates. Supply `Password` and
it is hashed (SHA-512 crypt, via the `openssl` that ships with Git for Windows)
at build time - the plain text never reaches the VM. To keep plain text out of
config files entirely, run `.\scripts\New-PasswordHash.ps1` and paste the result
into `PasswordHash` instead.

`Guest.AdditionalUsers` creates further accounts on first boot:

```powershell
AdditionalUsers = @(
    @{ Username = 'qa'; RealName = 'QA'; Password = 'ubuntu'; Sudo = $true; Groups = @('docker') }
)
```

### Disks

Three disks per VM, named by the drive letters production uses:

| Drive | Size | Config | Mounted at |
| --- | --- | --- | --- |
| `C:` | 25 GB | `DiskGB` | `/` (the system disk) |
| `E:` | 50 GB | `VM.DataDisks` label `drive-e` | `/mnt/e` |
| `L:` | 25 GB | `VM.DataDisks` label `drive-l` | `/mnt/l` |

```yaml
diskGB: 25                     # drive C:
vm:
  dataDisks:
    - label: drive-e
      driveLetter: E
      sizeGB: 50
    - label: drive-l
      driveLetter: L
      sizeGB: 25
```

Linux has no drive letters, so a `driveLetter` disk is mounted at `/mnt/<letter>`.
That deliberately avoids taking over a system directory: production has no mount
points, so nothing here assumes `/var/log` or `/opt` is a separate volume. Set
`mountPoint` to override.

All dynamically expanding, so they cost only what is written. Each is attached at
a fixed SCSI location, which is how the guest identifies drive `E:` instead of
guessing at `/dev/sdc`. Add, resize, or drop entries freely: a disk whose label
already exists is left alone.

Full detail, including growing a disk and giving Docker its own volume:
[docs/disks.md](docs/disks.md).

### Toolchain

`Bootstrap` flags decide what gets installed. Node comes from `nvm` (pinned
version, so every build matches) with `corepack` enabled for yarn and pnpm:

```powershell
Bootstrap = @{
    InstallNode   = $true
    NvmVersion    = 'v0.40.1'   # pinned
    NodeVersion   = 'lts/*'     # any nvm alias: 'lts/*', '22', 'v20.11.1'
    InstallDocker = $true
    InstallDdev   = $true       # ddev + a trusted mkcert local CA
    InstallVSCode = $true
    InstallChrome = $true       # google chrome, from Google's own repo
}
```

ddev comes from DDEV's own apt repo, so it upgrades with the rest of the system,
and `mkcert -install` runs for the primary user so ddev's HTTPS is trusted in
Firefox and Chrome. It needs `InstallDocker`, and the user is already in the
`docker` group.

Chrome is installed from Google's own repository rather than as Chromium, so it is
what the team tests against and it updates with the rest of the system afterwards.
It is on by default and skips itself on a machine with no desktop, so the
`ubuntu-server` blueprint does not pull a browser it cannot display.

### Reaching client infrastructure

`InstallRemoteAccess` is on by default and covers the four families you get handed
by different clients:

| | Tools |
| --- | --- |
| The open protocols | `openvpn`, `wireguard-tools` |
| Cisco AnyConnect and Palo Alto GlobalProtect | `openconnect` |
| Windows servers and remote desktops | `freerdp`, `remmina` |
| A subnet over a plain SSH login | `sshuttle` |

The command line tools go on every machine, so a headless box works as a jump
host. The NetworkManager VPN plugins and the RDP GUI are installed only where
there is a desktop, since importing a `.ovpn` from GNOME's network settings is the
point of them.

Optional packages are installed one at a time and a name that does not exist on
this release is reported rather than failing the step, because plugin package names
move between releases and a missing VNC plugin should not cost you the build.

For per-client separation, `ProjectName` is the lever: `clienta` and `clientb`
produce `clienta-fedora-workstation` and `clientb-fedora-workstation`, separate
machines with separate keys, and the build refuses to start if a name is already
taken.

### If you want to run VMs inside the VM

```powershell
VM = @{ NestedVirtualization = $true }
```

That sets `ExposeVirtualizationExtensions` and also turns on `MacAddressSpoofing`,
without which traffic from VMs running inside the guest is silently dropped by the
virtual switch. It forces static memory, because Hyper-V does not support nested
virtualisation alongside dynamic memory, and warns that it has done so. Budget the
RAM accordingly: whatever you set is claimed from the host for as long as the VM
runs, and each nested VM carves its own share out of that.

### GitHub access

**The easy path needs no config at all: drop your key files into `ssh_keys/`.**

```powershell
Copy-Item $HOME\.ssh\id_ed25519     .\ssh_keys\   # the guest's GitHub identity
Copy-Item $HOME\.ssh\id_ed25519.pub .\ssh_keys\
```

The build finds them. A private key becomes the guest's SSH identity; a
standalone `.pub` with no private half goes into the guest's `authorized_keys`
instead, so `ssh dev@fedora-dev.local` from this laptop needs no password. If the
`.pub` is missing it is derived with `ssh-keygen -y`. Nothing in that directory is
ever committed - it is git-ignored twice over. See
[ssh_keys/README.md](ssh_keys/README.md).

Everything below is optional on top of that - a VM builds and works with every
bit of it turned off:

```powershell
Git = @{
    UserName  = 'Scott Scarlett'
    UserEmail = 'scott@example.com'
}
GitHub = @{
    Username     = 'scottscarlett'
    SshKey       = @{ Mode = 'auto' }   # auto | generate | copy | none
    Cli          = @{ Install = $true; TokenFile = 'C:\Users\you\.secrets\gh-token' }
    UploadSshKey = $true
}
```

`Username` is optional. It sets `git config github.user` in the guest, and lets
bootstrap check that `ssh -T git@github.com` greets the account you expected
rather than a stale key belonging to someone else.

`Mode = 'auto'` (the default) is what makes the drop directory work: use a key
from `ssh_keys/` if there is one, otherwise generate a fresh keypair **inside the
guest** so no private key is written to the seed disk or needed on your laptop.
Either way the public half appears in the login banner ready to paste into
GitHub - or, with a token carrying the `admin:public_key` scope, it is registered
on your account automatically and the VM clones private repos with no manual step
at all. The token also signs `gh` in.

Copying a key does put the private half on the seed disk in plain text on the way
in; the build says so, bootstrap shreds the staged copy, and
[docs/github-access.md](docs/github-access.md) covers the trade-off and how to
detach the seed disk afterwards. `'generate'` forces a guest-side key even when
`ssh_keys/` holds one; `'none'` skips the identity entirely, though a laptop
`.pub` in `ssh_keys/` still lands in `authorized_keys`.

### Cloning repositories

Optional - empty by default, so nothing is cloned unless you ask. Give it repo
links and they are checked out on first boot:

```powershell
Repos = @{
    Root  = 'src'                              # relative to the user's home
    Items = @(
        'https://github.com/acme/web.git'      # clones to ~/src/web
        'git@github.com:acme/private-api.git'  # clones to ~/src/private-api
        @{
            Url       = 'https://github.com/acme/tools.git'
            Directory = 'team/tools'
            Branch    = 'develop'
            Depth     = 1
            Recurse   = $true                  # submodules
            PostClone = 'npm ci'               # run in the clone, as the user
        }
    )
}
```

An entry is a bare URL or a hashtable, so the common case stays a one-liner. This
runs after the SSH key and `gh` are set up, so private remotes work - and because
it merges like any other config, one machine in a fleet can have its own list.

Re-running is safe: an existing clone is **fetched, never reset or pulled**, so
uncommitted work is never at risk. URLs and directories are validated at build
time, so a typo fails during `-RenderOnly` rather than mid-install.

Detail, and how to make private `git@` clones work unattended:
[docs/repos.md](docs/repos.md).

### Databases

Optional, off by default. Two engines for now, each with a version you pick:

```powershell
DbEngine  = 'postgres'   # or 'mysql'. Blank installs neither.
DbVersion = ''           # blank takes the engine default
DbClient  = $true        # psql / mysql on the path
DbServer  = $true        # the server too, running as a service
```

| Engine | Default version | Default port | Versions accepted |
| --- | --- | --- | --- |
| `postgres` | 17 | 5432 | 13, 14, 15, 16, 17, 18 |
| `mysql` | 8 | 3306 | 8, 8.0, 8.4 |

Client and server are separate switches because they usually belong on different
machines: the application VM wants `psql` so it can reach the database, and only
the database VM runs the server. Asking for the server implies the client.

Postgres comes from the PostgreSQL project's own repository, which is what makes a
specific major version available on both Fedora and Ubuntu rather than whatever
that release happens to ship. MySQL comes from the distribution, so the requested
version is verified after install and a mismatch is reported rather than passing
silently.

`Database.ListenAddress = '*'` is what makes a server reachable from your other
VMs, and it does the four separate things that are all required for that to work:
binds beyond localhost, opens the port in firewalld, labels the port for SELinux
on Fedora, and adds the authentication rule the engine needs for remote clients.
A remote server with no `AdminPassword` is refused at build time.

`DbAdminer = $true` adds Adminer, the single-file database browser, and prints the
route it is served on:

```
    adminer     http://acme-fedora-workstation.local:3800/   (adminer 4.8.1)
```

Deliberately not the Docker image. It is served by PHP's own built-in web server
behind a systemd unit, so there is no nginx, no php-fpm socket path that differs
between the two distros, and no dependency on Docker being up before the GUI
works. That server is single threaded and not meant for production traffic, which
is the right trade for a single-user database GUI on a dev VM. Adminer has no
login of its own, so on a shared switch treat the route as internal.

Detail, the fleet example, Adminer, and where the admin password lives:
[docs/databases.md](docs/databases.md).

### Copying files in, and post-start hooks

Both optional, both from config. Carry a directory from this laptop into the
guest:

```powershell
CopyFrom = 'files'    # contents copied into the guest
CopyTo   = ''         # blank means the primary user's home
```

and run your own commands once everything else is provisioned:

```powershell
PostStart = @(
    'ddev config global --no-bind-mounts'
    'cd ~/src/web && npm ci'
)
```

The commands are rendered into one script and run as step 80, after the repos are
cloned so a hook can work inside a clone, and before the finalize step so its
result reaches the bootstrap report. It runs through a login shell as the primary
user by default, so `nvm` and `ddev` are on the path exactly as they would be
after logging in. `Hooks.RunAtEveryBoot` installs a systemd unit as well, so the
same script runs on every start.

Copied files travel base64-encoded inside the seed, so binaries and line endings
survive untouched, and directory structure is preserved. That also means the seed
carries them: over 10 MB warns, over 64 MB fails the build. It is for
configuration and certificates, not bulk data.

Both are reported from config before a build and from the guest after one:
[docs/files-and-hooks.md](docs/files-and-hooks.md).

### Fleets and name resolution

`Machines` in `vm.config.yaml` defines several VMs that inherit everything above
and override only what they name:

```powershell
Machines = @{
    'ubuntu-gui'           = @{ VM = @{ CpuCount = 6 } }
    'ubuntu-middleware-db' = @{
        Blueprint   = 'ubuntu-server'
        Bootstrap = @{ InstallDesktop = $false; ExtraAptPackages = @('postgresql-client') }
    }
}
```

```powershell
.\scripts\New-DevVM.ps1 -Machine ubuntu-middleware-db   # one
.\scripts\New-DevVM.ps1 -All                            # the whole fleet
```

### Naming, and the address you browse to

`ProjectName` names the work, not the OS. The VM and the guest hostname both
default to `<ProjectName>-<Blueprint>`, and avahi publishes that hostname, so it
is also the address a browser uses:

```powershell
ProjectName = 'acme'          # with Blueprint = 'fedora-workstation'
#  VM name and hostname:  acme-fedora-workstation
#  browse to:             http://acme-fedora-workstation.local
```

Change `ProjectName` and the whole project is renamed together. An explicit
`VmName` or `Hostname` overrides it, and a `Machines` key beats both.

**A real build refuses to start if the name is taken**, which covers two
collisions: a Hyper-V VM already using that name (`-Force` replaces it), and
`<hostname>.local` already answering on the network. The second is not a Hyper-V
question, since the name might belong to a teammate's laptop on the same switch,
and it matters because avahi renames the loser to `<hostname>-2.local` so the
address you hand someone reaches the wrong machine. `-RenderOnly` reports it as a
warning instead, and `-All` also refuses two machines that resolve to the same
name.

Names are validated as DNS labels at render time, because one that is not a valid
label will not resolve over mDNS.

### Fleet name resolution

Every machine defaults to the same virtual switch and gets `avahi-daemon` plus
`libnss-mdns` from `Bootstrap.EnableMdns`, so they advertise themselves in the
`.local` mDNS domain and find each other **without static IPs or hosts files**:

```bash
ubuntu-gui$ ping ubuntu-middleware-db.local
ubuntu-gui$ psql -h ubuntu-middleware-db.local -U app
```

Windows 10 and later resolve `.local` too, so the host reaches the guests the
same way - which is why `Get-DevVMStatus.ps1` and the SSH examples all use
`<hostname>.local` rather than a DHCP address that changes on every reboot.

The one requirement is a **shared switch**: mDNS is link-local multicast, so
machines on different switches will not see each other.

---

## Day-to-day

```powershell
.\vm.ps1 open   -Machine ubuntu-gui   # start it if needed, then open the desktop
.\vm.ps1 status                       # what each machine is and how it is doing
.\vm.ps1 status -Follow               # live
.\vm.ps1 status -Report               # the guest's own step-by-step report
.\vm.ps1 stop                         # shut it down, keep everything
.\vm.ps1 stop   -Save                 # hibernate instead, resumes in seconds
.\vm.ps1 start  -Wait                 # bring it back, wait until reachable
.\vm.ps1 remove -Machine ubuntu-gui -DeleteDisks
```

The full command list is in [the commands](#the-commands) at the top, kept in one
place so the two cannot drift apart.

### Just open the desktop

One command from a cold laptop to a desktop you can click around in:

```powershell
.\scripts\Open-DevVM.ps1 -Machine ubuntu-gui
```

It starts the VM if it is not running, waits until the guest is actually up, and
then opens the console. The waiting is the whole point. Hyper-V decides between a
basic session and an Enhanced Session at the moment the window opens, so a window
opened while the guest is still booting is stuck at a fixed size with no clipboard
and has to be closed and reopened. Waiting first means the window you get is the
good one.

It reads the guest's readiness from the host, using the heartbeat integration
service and the presence of an address, so it needs no SSH key and no network path
into the VM. If the machine does not exist it tells you to run `New-DevVM.ps1`
rather than quietly building one, and if `vmconnect.exe` is missing it names the
Windows feature to enable.

### Stopping and starting

A VM you are not using should not be costing you 12 GB of RAM. Shutting it down
releases the CPU and memory and keeps everything else: disks, files, repos,
installed packages, the whole provisioned state.

```powershell
.\scripts\Stop-DevVM.ps1            # ask the guest to shut down cleanly
.\scripts\Stop-DevVM.ps1 -Save      # write memory to disk, resume where you were
.\scripts\Stop-DevVM.ps1 -TurnOff   # cut the power, for a guest that will not go
.\scripts\Start-DevVM.ps1 -Wait     # start it and wait until it has an address
```

Both take `-Machine`, `-Name` and `-All`, resolved exactly as `New-DevVM.ps1`
resolves them.

**Starting never re-provisions.** `vm-bootstrap.service` carries
`ConditionPathExists=!/var/lib/vm-bootstrap/complete`, so once a clean run has
happened the unit is skipped on every later boot. Your work comes back untouched.
Rebuilding from scratch is `New-DevVM.ps1 -Force`, a different script on purpose.

**Nothing here deletes a disk.** Only `Remove-DevVM.ps1 -DeleteDisks` does, and it
needs that flag. The one action that would lose your work is restoring the
`clean-install (pre-bootstrap)` checkpoint, which puts the machine back to a bare
OS install. Take your own checkpoint before anything risky:

```powershell
Checkpoint-VM -Name acme-fedora-workstation -SnapshotName 'before the upgrade'
```

A graceful shutdown depends on the guest cooperating, so it is polled with a
deadline rather than waited on forever. If the guest ignores the request the script
says so and tells you to use `-TurnOff`, instead of hanging.

Two things worth knowing about resources. Disks are dynamically expanding, so they
only ever cost what was written, and stopping does not shrink them:
`Optimize-VHD -Mode Full` reclaims space once the VM is off. And an idle VM is
cheaper than it looks, since dynamic memory hands RAM back down toward
`MemoryMinimumGB`, but a stopped one is free.

The status script answers "what is this machine, where do I reach it, and how is
it doing?" in one screen:

```
  acme-fedora-workstation        Running  up 00:42:11
    os          Fedora Linux 42 (Workstation Edition)
    kernel      6.14.0-63.fc42.x86_64
    blueprint   fedora-workstation
    address     http://acme-fedora-workstation.local   (resolves to 172.28.144.3)
    ssh         ssh dev@acme-fedora-workstation.local
    user        dev (Developer)   groups: dev wheel docker
    hardware    6 vCPU   12.0 GB RAM assigned   dynamic 4.0 GB to 16.0 GB
    guest up    42 minutes
    disks       DRIVE   LABEL           ALLOCATED    ON HOST  MOUNT
                C:      system            25.0 GB     8.4 GB  /
                        in use  6.1 GB of 25.0 GB  (26%)
                E:      drive-e           50.0 GB     1.2 GB  /mnt/e
                        in use  123 MB of 49.0 GB  (1%)
                L:      drive-l           25.0 GB     614 MB  /mnt/l
                        in use  62 MB of 24.5 GB  (1%)
    github      authenticated as scottscarlett
    bootstrap   complete
```

Three sources, each answering what only it can. Hyper-V is authoritative for CPU,
memory and the disks that are really attached, so drift from config shows up
rather than hiding. Config supplies what the hypervisor cannot know: the
blueprint, the drive letters, and which mount point each disk lands on. The guest
supplies the rest over SSH, which is why `os` reads as the release actually
installed rather than the one requested, and why `ALLOCATED` (the disk as the
guest sees it), `ON HOST` (what the growing VHDX costs today) and `in use` are
three different numbers.

It degrades rather than failing: without SSH access you still get the whole
host-side picture, and a machine that has not been built yet is described from
config so you can see what would be created.

Inside the guest, bootstrap is re-runnable and remembers what it finished:

```bash
sudo vm-bootstrap --list          # per-step state
sudo vm-bootstrap                # run whatever has not completed
sudo vm-bootstrap --only 32-docker --force
tail -f /var/log/vm-bootstrap.log
```

A step that fails does not strand the machine - the runner reports it, carries
on, and leaves that step un-stamped so a re-run picks it up.

When the run finishes it prints a report of every step with its status and
timing, and writes the same page to `/var/log/vm-bootstrap-report.txt`:

```
 STEP                   STATUS   TIME
 ---------------------- -------- -----
 32-docker              FAILED   11s
 33-ddev                ok       19s

 12 step(s): 11 ok, 1 failed, 0 skipped   total 383s

 FAILED: 32-docker
 Re-run just those:
     sudo vm-bootstrap --only 32-docker --force
```

So the two files to reach for are the report for what broke, and
`/var/log/vm-bootstrap.log` for the full output of the step that broke. The
login banner names both. See [docs/troubleshooting.md](docs/troubleshooting.md).

### Enhanced Session

`VM.EnhancedSession` sets the VM's transport to `HvSocket` on the host and
installs xrdp bound to a vsock in the guest, which gives you a resizable
desktop, a shared clipboard, and drive redirection with **no network path to the
guest at all**. Reconnect the VM window after the post-bootstrap reboot and log
in through the xrdp prompt.

For a basic session instead, `Bootstrap.DisplayResolution` pins the Hyper-V
framebuffer (default `1920x1080`); without it Ubuntu comes up at 1024x768 and
cannot be changed from inside the guest.

### Sharing a built machine

The repo is the shareable artifact - anyone who clones it builds the same VM. To
hand over an already-built one, export it:

```powershell
Export-VM -Name fedora-dev -Path D:\vm-exports
```

---

## The blueprints that ship

| Blueprint | `OsVersion` | Installer | Seed label | Notes |
| --- | --- | --- | --- | --- |
| `fedora-workstation` | `43` | Anaconda kickstart | `OEMDRV` | Fedora + GNOME. The default. |
| `redhat-rocky-workstation` | `9` | Anaconda kickstart | `OEMDRV` | Rocky Linux + GNOME. Binary-compatible with RHEL 9. |
| `redhat-rhel-workstation` | `9.7` | Anaconda kickstart | `OEMDRV` | Red Hat Enterprise Linux itself. **Needs your own DVD and a subscription.** |
| `ubuntu-desktop` | `24.04` | cloud-init autoinstall | `CIDATA` | Ubuntu Server ISO, GNOME on first boot |
| `ubuntu-server` | `24.04` | cloud-init autoinstall | `CIDATA` | inherits `ubuntu-desktop`, headless |

`Blueprint` and `OsVersion` sit together at the top of the config, because a
version only means something to its own OS: `43` is Fedora, `24.04` is Ubuntu. A
mismatch fails at render time rather than 404ing on a download. Blank `OsVersion`
and each blueprint supplies its own.

**Fedora has no LTS** - about 13 months per release, so a Fedora workstation wants
upgrading roughly yearly, and this is not just paperwork: once a release is
retired Fedora moves it off the mirrors into `pub/archive`, and the build then
fails on a 404 when it goes to fetch the ISO. If that happens, bump `OsVersion`.

`redhat-rocky-workstation` is the one that does not need that. Rocky Linux is a
free, binary-compatible rebuild of Red Hat Enterprise Linux, on the RHEL 9
lifecycle: full support to 2027 and maintenance to May 2032. Pick it if your
client servers run RHEL, CentOS, Rocky or Alma and you want the dev machine to
match them. Ubuntu 24.04 LTS is supported to April 2029.

`redhat-rhel-workstation` is Red Hat's own build, for when a rebuild will not do:
support, compliance, or reproducing a customer's exact platform. It is the one
blueprint that cannot build unattended out of the box, and both reasons are
commercial rather than technical.

- **The ISO cannot be downloaded.** Red Hat's media is behind an account, so
  nothing here can fetch or checksum it. Download the **binary DVD** yourself
  from [developers.redhat.com](https://developers.redhat.com/products/rhel/download)
  (a no-cost Developer subscription is enough) and set `isoPath`. Take the DVD,
  not the boot ISO: the boot ISO carries the installer and no packages, and
  expects an entitled CDN mid-install.
- **A fresh RHEL has no repositories.** Until `subscription-manager` registers
  the machine, dnf can install nothing beyond the DVD. That costs EPEL, and
  therefore xrdp, and therefore Enhanced Session. Put an activation key in your
  local config; the build warns at plan time if you have not.

Everything below that layer is shared with the Rocky blueprint, because they are
the same operating system.

A `Machines` entry can name its own blueprint and version, so one fleet can mix
distros. All they need to share is the virtual switch, which is all mDNS needs.

### How Fedora differs, and where that lives

Both installers are driven the same way, and the disk **label** is the whole
contract: cloud-init looks for `CIDATA` and reads `user-data`, Anaconda looks for
`OEMDRV` and reads `ks.cfg`. Neither needs ISO repacking, an HTTP server, or a
keystroke at the boot menu.

Everything else that differs is confined to the blueprint:

| | Ubuntu | Fedora |
| --- | --- | --- |
| unattended format | autoinstall YAML | kickstart |
| payload delivery | `write_files` | base64 heredocs in `%post` |
| packages | `apt` | `dnf` |
| admin group | `sudo` | `wheel` |
| firewall | ufw, inactive | firewalld, active |
| SELinux | absent | enforcing |
| python headers | `python3-dev` | `python3-devel` |
| mDNS | `libnss-mdns` | `nss-mdns` |
| Hyper-V tools | `linux-tools-virtual` | `hyperv-daemons` |
| polkit rules | `.pkla` | JS `.rules` |

**Every step lives once, in `blueprints/bootstrap/`, and runs on both.** There are
no per-OS copies to drift apart. `_lib.sh` supplies `pkg_install`, `admin_group`,
`firewall_allow_service`, `selinux_restore` and friends, which is all most steps
need. Where the package names genuinely differ, the step branches and writes both
lists out:

```bash
case "$PKG_MANAGER" in
    apt)     pkg_install python3-dev python3-venv python3-pip pipx ;;
    dnf|yum) pkg_install python3 python3-devel python3-pip pipx ;;
esac
```

Two lists side by side, not a translation table. Seeing `python3-devel` next to
`python3-dev` is the point: a table that quietly maps one to the other hides
exactly the cases worth looking at.

A blueprint still keeps its own directory for the things that are genuinely its
own: the seed template, and assets like `xrdp-startwm.sh` where the content
differs rather than the name. It can also override any shared step by dropping a
file of the same name in its own `bootstrap/`, since later directories win when
the trees are layered. That is the escape hatch for an OS that needs different
*logic*, not different names.

---

## Changing the OS

A **blueprint** is the recipe for one kind of machine: where its install media
comes from, the unattended-install template, the firmware it needs, and the
bootstrap steps that provision it. Two ship here, `ubuntu-desktop` and
`ubuntu-server`, and `Blueprint` in config picks one.

Each blueprint declares the release it installs, so switching OS is one key
(`Blueprint`) and switching release is another (`Release`). A different distro is a
new directory under `blueprints/`. Nothing in `scripts/lib/HyperV.psm1` knows the
guest is Ubuntu apart from the Secure Boot template the blueprint names.

See [docs/adding-an-os.md](docs/adding-an-os.md).

---

## Requirements

- Windows 10/11 **Pro** or Enterprise with Hyper-V enabled
  (`Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -All`)
- An **elevated** PowerShell session (5.1 or 7+; the Hyper-V cmdlets need admin)
- Git for Windows - used for `openssl` when hashing passwords
- Free disk space for the disks (all dynamically expanding, so far less than the
  290 GB they add up to) plus roughly 3 GB for the cached ISO

Something not working? [docs/troubleshooting.md](docs/troubleshooting.md).

## Layout

```
vm.ps1                         the front door: .\vm.ps1 <command> [args]
vm.config.yaml                 team standard, committed
vm.config.local.example.yaml   copy to vm.config.local.yaml (git-ignored)
ssh_keys/                      drop key files here (git-ignored)
blueprints/                    one directory per kind of machine
  bootstrap/                   every step, shared by all blueprints
  fedora-workstation/          kickstart + dnf (the default)
  redhat-rocky-workstation/    kickstart + dnf, RHEL-compatible
  redhat-rhel-workstation/     kickstart + dnf, real RHEL (own ISO + subscription)
  ubuntu-desktop/              autoinstall + apt
  ubuntu-server/               inherits ubuntu-desktop, headless
scripts/
  New-DevVM.ps1                build
  Open-DevVM.ps1               start if needed, wait, open the desktop
  Start-DevVM.ps1              start or resume; never provisions
  Stop-DevVM.ps1               shut down, save state, or turn off. Keeps the disks
  Remove-DevVM.ps1             tear down
  Get-DevVMStatus.ps1          what each machine is, address, hardware, disks, bootstrap
  New-PasswordHash.ps1         SHA-512 crypt helper
  lib/                         Common / Yaml / Iso / Seed / HyperV modules
docs/
  configuration.md             every setting, the shortcuts, the YAML subset
  disks.md                     the three-disk layout, growing and moving disks
  github-access.md             SSH keys, tokens, and the secret trade-offs
  repos.md                     optional repo cloning
  adding-an-os.md              new blueprints, new bootstrap steps
  troubleshooting.md
build/                         generated, git-ignored
```
