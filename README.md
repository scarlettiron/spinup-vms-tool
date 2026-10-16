# Vmseed

**Reproducible dev VMs from a config file.** · [Website](https://scarlettiron.github.io/vmseed/) ·
[Docs](https://scarlettiron.github.io/vmseed/docs/) · [Design](docs/vmseed-design.md)

[![CI](https://github.com/scarlettiron/vmseed/actions/workflows/ci.yml/badge.svg)](https://github.com/scarlettiron/vmseed/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/scarlettiron/vmseed?include_prereleases&sort=semver)](https://github.com/scarlettiron/vmseed/releases)

Vmseed builds fully provisioned Linux development VMs from a YAML file. Describe the machine once,
run one command, and about half an hour later you're logged into a desktop with git, Node, Python,
Docker, ddev, VS Code, a database and your repos already cloned. It's the same build on every
laptop on your team. It works like ddev: run `vmseed init` in a project folder, then start, stop,
open and list your VMs from anywhere. Vmseed is a single binary for Windows (Hyper-V), Linux
(QEMU/KVM) and macOS (Apple Virtualization), installed with winget, Chocolatey, Homebrew or apt,
with no runtime to install; it drives the hypervisor your OS already has.

> [!NOTE]
> **Vmseed is 0.x.** Every command is implemented on all three backends. Linux/libvirt has a
> nightly end-to-end build in CI; Hyper-V and Apple Virtualization are unit-tested and get their
> first real-hardware pass before 1.0. The roadmap is in [docs/vmseed-design.md §16](docs/vmseed-design.md#16-milestones).

## Install

**Windows**

```powershell
winget install CodeScarlett.Vmseed
# or
choco install vmseed
```

**macOS and Linux (Homebrew)**

```bash
brew install scarlettiron/tap/vmseed
```

**Debian and Ubuntu (apt)**

```bash
curl -fsSL https://scarlettiron.github.io/vmseed/apt/vmseed-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/vmseed-archive-keyring.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/vmseed-archive-keyring.gpg] https://scarlettiron.github.io/vmseed/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/vmseed.list
sudo apt update && sudo apt install vmseed
```

**Anything else:** download an archive for your platform from
[Releases](https://github.com/scarlettiron/vmseed/releases), check it against `SHA256SUMS`, and put
`vmseed` on your `PATH`. Every release file also carries a GitHub build-provenance attestation:
`gh attestation verify <file> --repo scarlettiron/vmseed` confirms it was built by this
repository's release workflow.

Then run `vmseed doctor` to check your machine is ready.

## Requirements

| Host | You need |
| --- | --- |
| Windows 10/11 **Pro, Enterprise or Education** | Hyper-V turned on, and your account in the **Hyper-V Administrators** group (`Add-LocalGroupMember -Group 'Hyper-V Administrators' -Member $env:USERNAME`, then sign out and in). Windows Home has no Hyper-V. |
| Linux | `/dev/kvm`, libvirt, QEMU and OVMF (`sudo apt install qemu-system libvirt-daemon-system ovmf`, or `sudo dnf install @virtualization`), and your user in the `libvirt` group. Optional: `virt-viewer` for the console, FreeRDP or Remmina for the desktop. |
| macOS 13+ (Apple Silicon or Intel) | `brew install vfkit` (Homebrew installs it with Vmseed), and Microsoft's **Windows App** from the App Store to open the desktop. Apple Silicon runs arm64 guests. |
| All | `ssh` (built into Windows 10+, macOS and Linux). |

## Quick start

```bash
cd ~/work/my-project
vmseed init       # project name, blueprint, your user and password
vmseed create     # download the ISO, install unattended, provision (15-40 minutes)
vmseed open       # start it if needed and open the desktop
```

`init` creates `.vmseed/` in the folder:

```
.vmseed/
  vm.config.yaml                 project settings: commit this
  vm.config.local.yaml           your user and password hash: git-ignored
  vm.config.example.yaml         every option, documented
  vm.config.local.example.yaml
  ssh_keys/                      drop .pub files here
  .gitignore
```

Your password is stored only as a SHA-512 crypt hash. Edit `vm.config.yaml` to choose what goes in
the VM: CPUs, memory, disks, database, repos to clone. `vmseed config --validate` checks it and
`vmseed create --render` shows exactly what would be installed without building anything.

Once it's built:

```bash
vmseed status --follow  # watch bootstrap finish
vmseed ssh              # a shell in the VM (Vmseed adds its own key, no setup)
vmseed exec -- docker ps # one command
vmseed list             # every VM on this computer, with status and URLs
vmseed stop             # get the memory back
```

Each VM is published as `http://<project>-<blueprint>.local` over mDNS.

## Commands

Run `vmseed`, `vmseed help` or `vmseed -h` for the full list, and `vmseed help <command>` for any one
command. `vmseed -v` prints the version.

| Command | What it does |
| --- | --- |
| `init` | create `.vmseed/` config in this folder |
| `config` | show the merged config; `--validate` runs every check `create` runs |
| `create` (`new`, `build`) | build the VM from config; `--render` to preview, `--force` to rebuild, `--no-checkpoint` |
| `start` (`up`) | start or resume; never provisions |
| `stop` (`halt`, `down`) | shut down, keeping the disks; `--save` (not on macOS), `--force` |
| `restart` | stop, then start |
| `poweroff` | stop every VM on this computer |
| `remove` (`rm`, `destroy`) | delete the VM; `--delete-disks` removes its disks too |
| `open` (`desktop`, `console`) | open the desktop; `--console` for the raw console, `--no-wait` |
| `ssh` | a shell in the VM |
| `exec -- <cmd>` | run one command in the VM |
| `logs` | bootstrap and post-start logs; `-f` to follow, `-n` lines |
| `list` (`ls`) | every VM with status and URLs; `--live` adds database and bootstrap, `--rescan` rebuilds the list |
| `status` (`info`, `describe`) | full detail for a VM; `--follow`, `--no-ssh`, `--report` |
| `snapshot` | `create`, `list`, `restore`, `delete` |
| `blueprints` | list blueprints; `blueprints new` to make your own |
| `hash` | make a SHA-512 crypt password hash |
| `doctor` | check this computer is ready to run VMs |
| `clean` | delete cached ISOs and leftover build files |
| `completion` | shell completion for bash, zsh, fish, PowerShell, elvish |
| `version`, `help` | version and help |

VM commands take a VM or machine name, or act on the project in the current folder. `start`,
`stop`, `restart`, `remove` and `status` also take `--all`. Global options: `-h`/`--help`,
`-v`/`-V`/`--version`, `--verbose`, `-q`/`--quiet`, `--json`, `--no-color`, `-y`/`--yes`.

## Platform support

| Host | Hypervisor | Guests | Status |
| --- | --- | --- | --- |
| Linux | QEMU/KVM via libvirt | x86_64, arm64 | ✅ built; nightly end-to-end build in CI |
| Windows 10/11 Pro, Enterprise, Education | Hyper-V | x86_64 | ✅ built; first real-hardware pass before 1.0 |
| macOS 13+ (Apple Silicon and Intel) | Apple Virtualization via vfkit | arm64 on Apple Silicon, x86_64 on Intel | ✅ built; first real-hardware pass before 1.0 |

Blueprints that ship: Rocky Linux (the default), RHEL, Fedora and Ubuntu with GNOME, and Ubuntu
Server. Each installs on x86_64 and arm64. You can copy any of them into your project and change
it with `vmseed blueprints new`.

## Guides

- [Configuration](docs/configuration.md): layers, quick settings, machines, Red Hat subscriptions
- [Disks](docs/disks.md), [databases](docs/databases.md), [repos](docs/repos.md),
  [GitHub access](docs/github-access.md), [files and hooks](docs/files-and-hooks.md)
- [Adding or changing an OS](docs/adding-an-os.md): blueprints and bootstrap steps
- [Troubleshooting](docs/troubleshooting.md)

This repository started as a PowerShell tool for Windows and Hyper-V (`vm.ps1` and `scripts/`).
Vmseed replaced it and it was removed before 1.0; it remains in the git history.
`vmseed init --import <folder>` reuses config files from an old checkout of it.

## Learn more

- [Website and docs](https://scarlettiron.github.io/vmseed/)
- [Design and roadmap](docs/vmseed-design.md)
- [Changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md): setup, conventions, adding a blueprint
- [Security policy](SECURITY.md): please report vulnerabilities privately
- [Code of conduct](CODE_OF_CONDUCT.md)

## Author

Made by **Scarlett A. Scott** (codescarlett):
[GitHub @scarlettiron](https://github.com/scarlettiron) · [codescarlett.com](https://codescarlett.com)

## License

[MIT](LICENSE) © 2026 Scarlett A. Scott (codescarlett)
