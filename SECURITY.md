# Security policy

Vmseed builds virtual machines from a config file. Along the way it handles password hashes,
SSH keys and GitHub tokens, downloads operating system images, and drives a hypervisor that
needs elevated rights. A bug here can leak a credential or hand someone a machine they
shouldn't have. Reports are welcome and taken seriously.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Use GitHub's private reporting instead:
[**Report a vulnerability**](https://github.com/scarlettiron/vmseed/security/advisories/new).
It opens a private thread with the maintainer, and nothing is visible publicly until there is a
fix.

What helps most in a report:

- the output of `vmseed --version`, and your host OS and hypervisor
- the blueprint involved, if any
- what an attacker can do with it, and what they need first (access to your machine? to your
  project folder? to the same network as the VM? nothing?)
- the smallest set of steps that shows it, ideally starting from `vmseed init` in an empty folder
- the relevant part of your config, **with passwords, hashes, tokens and keys removed**

You will get a first reply within **7 days**. If a report is confirmed, the fix ships in a patch
release and the advisory credits you unless you would rather stay anonymous.

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |
| < 0.1   | No        |

Vmseed is pre-release software with one maintainer, so support means the latest 0.1.x release.
Upgrade before reporting if you can.

## What is in scope

The `vmseed` binary, the blueprints and guest bootstrap scripts it ships, the release artifacts,
and the package repositories it is published through. In particular:

- **Passwords and password hashes.** `vmseed init` and `vmseed hash` turn a password into a
  SHA-512 crypt hash, and only the hash is written to `vm.config.local.yaml`. A way to make
  Vmseed write a plain-text password to disk, log it, or print it is in scope.
- **Seed disks.** The seed disk carries the password hash and the bootstrap payload into the VM.
  It is stored under `~/.vmseed/vms/<name>/`, deleted with the VM, and shredded inside the guest
  once bootstrap finishes. Anything that leaves it readable to other users on the host, leaves
  it attached after bootstrap, or skips the shred is in scope.
- **SSH keys.** Public keys dropped into `.vmseed/ssh_keys/` become the VM user's
  `authorized_keys`. The folder's `.gitignore` keeps private keys out of git. A private key
  reaching git, the seed disk, the VM, or a log is in scope.
- **GitHub tokens.** `GitHub.Cli.Token` and `GitHub.Cli.TokenFile` hand a token to the guest so
  it can authenticate the GitHub CLI and upload an SSH key. A token being logged, written
  somewhere world-readable, or left behind in the guest after it has been used is in scope.
- **Download integrity.** Install ISOs are checked against the distribution's published
  checksum before use. A way to get Vmseed to boot an image that doesn't match, or to skip the
  check without the user asking, is in scope.
- **Privilege.** Hyper-V needs an administrator or a member of the Hyper-V Administrators group,
  and libvirt needs the `libvirt` group. Vmseed never elevates itself. Anything that lets a less
  privileged user on the host use Vmseed to act with those rights is in scope.
- **Release artifacts.** Every release publishes a `SHA256SUMS` file, and the winget, Chocolatey
  and Homebrew packages pin those checksums. A mismatch between a published archive and its
  checksum, or a package that installs something other than the release archive, is in scope.
- **The apt repository.** The repository at `https://scarlettiron.github.io/vmseed/apt` is signed,
  and installs are expected to use the published keyring with `signed-by`. A way to get an
  unsigned or wrongly signed package accepted is in scope.

## What is out of scope

- Vulnerabilities in the hypervisor itself (Hyper-V, QEMU/KVM, libvirt, Apple Virtualization,
  vfkit), or in the operating systems the blueprints install. Report those upstream.
- Anything that needs an attacker to already be an administrator on the host, or to already
  have write access to your project's `.vmseed/` folder, since both are trusted by design.
- The plain `password` config key. It still works for compatibility, it is documented as unsafe,
  and Vmseed warns when it appears in the committed `vm.config.yaml`.
- What you choose to install inside the VM through config: extra packages, dotfiles repos,
  post-start hooks, or repos you clone.
- Default VM settings that are documented as convenient rather than hardened, such as password
  SSH login in a development VM on a NAT network, unless you can show an impact beyond the VM.

## Things worth knowing when you look

- **The local config file is the secret one.** `vm.config.yaml` is meant to be committed and
  shared. `vm.config.local.yaml` holds your identity and is git-ignored by the `.gitignore` that
  `vmseed init` writes into `.vmseed/`.
- **The hash is plain text on the seed disk until bootstrap runs.** That is the same as any
  cloud-init or kickstart install. It is shredded inside the guest afterwards, and the seed file
  on the host goes when the VM is removed.
- **Vmseed calls the system `ssh`**, so it uses your own keys, agent and `~/.ssh/config`. It
  connects with `BatchMode=yes` and never prompts for, stores, or forwards a password.
- **Guest scripts run as root inside the VM**, on first boot. They come from the binary or from
  your project's `.vmseed/blueprints/`, never from the network.

## Keeping your own setup safe

- Use `vmseed init` or `vmseed hash` and keep `passwordHash` in `vm.config.local.yaml`. Don't put
  `password` in `vm.config.yaml`.
- Prefer `GitHub.Cli.TokenFile` over an inline `GitHub.Cli.Token`, keep the file outside the
  project, and give the token only the scopes it needs.
- Only put public keys (`.pub`) in `.vmseed/ssh_keys/`.
- Verify downloads you install by hand against the release's `SHA256SUMS`.
- On Debian and Ubuntu, add the apt repository with the published keyring and `signed-by`, as
  the install instructions show, rather than trusting the key system-wide.
