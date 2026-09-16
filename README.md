<!--
  Scarlett A. Scott
  Created 2026-09-15
-->

# dev-vm

A Rocky Linux desktop VM on Hyper-V, built from config. GNOME, git, Node,
Python, Docker, ddev, VS Code, Chrome and Postgres — same build on every laptop.

```powershell

Copy-Item vm.config.local.example.yaml vm.config.local.yaml
# edit it: username, password, your name

# optional: the SSH key you already use for GitHub
Copy-Item $HOME\.ssh\id_ed25519* .\ssh_keys\

# from an ELEVATED PowerShell:
.\vm.ps1 create
```

Thirty to forty minutes later you can log in on the first create as the os iso will need to download. After the iso is downloaded it's an average of 10 mibutes. The guest writes every step to
`/var/log/vm-bootstrap-report.txt`; `.\vm.ps1 status -Report` prints it.

## Commands

| Command | What it does | Also |
| --- | --- | --- |
| `create` | build the VM from config | `new`, `build` |
| `open` | start if needed, then open the desktop | `desktop`, `console` |
| `start` | start or resume; never provisions | `up` |
| `stop` | shut down, keep the disks | `halt`, `down` |
| `status` | what it is, where to reach it, how it's doing | `info` |
| `remove` | delete it; needs `-DeleteDisks` to touch the disks | `destroy`, `rm` |
| `hash` | make a SHA-512 crypt hash for `passwordHash` | |

Arguments pass through, so any flag the underlying script takes works
(`status -Report`, `stop -Save`, `remove -DeleteDisks`). Everything except
`hash` needs elevation, because the Hyper-V cmdlets do.

## Configuration

`vm.config.yaml` is shared and committed. Yours goes in
`vm.config.local.yaml` — git-ignored, and only the keys you want to differ:

```yaml
username: scarlett
password: something-only-you-know
projectName: scott
```

`vm.config.example.yaml` lists every setting with its default.

- **Name.** `projectName` + blueprint, so `scott` gives
  `scott-redhat-rocky-workstation`, reachable at that name plus `.local`.
- **Disks.** 25 GB system, plus `E:` (50 GB) and `L:` (25 GB) at `/mnt/e` and
  `/mnt/l`. Dynamically expanding. Change them under `vm.dataDisks`.
- **GitHub.** A private key in `ssh_keys/` gets installed; add the `.pub` and you
  can SSH in without a password. With no key the guest makes one and prints it.
- **Database.** Postgres 17, Adminer on 3800. The link `status` prints is
  pre-filled except the password — set `dbAdminPassword` or Adminer can't get in.
- **Enhanced Session.** Resizable window and shared clipboard over RDP. Use
  `.\vm.ps1 open`, then pick `Xorg` at the xrdp login.

## Requirements

Windows 10/11 **Pro** or Enterprise with Hyper-V
(`Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -All`),
an elevated PowerShell 5.1+, and Git for Windows for its `openssl`. Budget
~1.5 GB for the cached ISO on top of the disks.
