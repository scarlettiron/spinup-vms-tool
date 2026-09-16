# dev-vm — tutorial narration script

Target: ~5 minutes. `[ ]` lines are screen cues, not spoken.

---

## 1. What this is  (~40s)

[ repo root open in VS Code ]

"This is a tool for building dev VMs on Hyper-V from config. Not one VM — any
VM. You describe the machine you want in a YAML file, run one command, and ten
minutes later you're logged into it.

The reusable part is the blueprint. A blueprint is one operating system and how
to install it unattended — Rocky, Fedora, Ubuntu, whatever you need. The
machine you actually get is 'this blueprint, plus my config.' Same config
against a different blueprint gives you the same dev environment on a different
OS.

The one shipped here is `redhat-rocky-workstation` — Rocky 9 with GNOME — so
that's what we'll build today. But nothing in the config file we're about to
walk through is Rocky-specific.

And what you get is fully set up: git, Node, Python, Docker, ddev, VS Code,
Chrome, Postgres, your repos already cloned. Same build on my laptop as on
yours. The point is that nobody hand-installs a dev box any more, and nobody
ends up with a box that's subtly different from everyone else's."

## 2. Blueprints  (~45s)

[ blueprints/ expanded, then blueprints/redhat-rocky-workstation/ ]

"So — blueprints. One directory per OS, and you pick one in your config.

Open the Rocky one. `blueprint.psd1` is the whole definition: where to fetch
the ISO and how to verify its checksum, which firmware and Secure Boot
settings, which packages the installer lays down, and the OS versions it will
accept. Next to it, a kickstart template — that's the answer file the installer
reads so nobody has to click through setup. A Debian-family blueprint would
have a cloud-init template there instead. Different mechanism, same slot.

Then there's `blueprints/bootstrap`, outside any single blueprint. Numbered
shell scripts that run inside the guest on first boot and install everything
else — desktop, Node, Python, Docker, the database. Those are shared across
every blueprint. They branch on the OS family where they have to, so adding a
blueprint mostly means describing the OS, not rewriting the toolchain.

We're not opening those today. The takeaway: blueprint is the OS, bootstrap is
the toolchain, and adding support for another OS is a new folder here — it
doesn't touch anything else."

## 3. The two config files  (~60s)

[ vm.config.yaml side by side with vm.config.local.example.yaml ]

"Two config files, and the distinction matters.

`vm.config.yaml` is the team file. It's committed. This is what we all agree
on — Rocky 9, six CPUs, ten gigs of RAM, Postgres 17, this VS Code extension
list, these repos. If you change this, you're changing it for everyone.

`vm.config.local.yaml` is yours. It's git-ignored. You copy
`vm.config.local.example.yaml` to it and put in only the keys you want
different — your username, your password, your project name. Everything you
leave out falls back to the team file.

So: your password never goes in the committed file. Your extra VS Code
extensions don't get pushed at everyone. And if you want sixteen gigs of RAM
because you've got the laptop for it, that's a one-line override on your side.

There's also `vm.config.example.yaml`, which lists every setting that exists
with its default. That's the reference when you're wondering 'can I change X.'"

## 4. Walking vm.config.yaml  (~90s)

[ scroll vm.config.yaml top to bottom, pausing on each ]

"Top of the file — `blueprint` and `osVersion`: which OS. `projectName` is
how the VM gets named: project name plus blueprint, so `alcon` gives
`alcon-redhat-rocky-workstation`, and that's also the hostname — you can reach
it at that name dot local.

`username`, `password`, `realName` — the account created inside the guest.
In real use, password lives in your local file, not here.

`cpus`, `memoryGB`, `diskGB` — the hardware.

`repos` and `reposRoot`. This is the good part: list a git URL here and it's
already cloned when you log in. Ours clone to `/mnt/e/WXS` — which, since this
is Linux, isn't a drive letter, it's a mount point. More on that in a second.

`dbEngine`, `dbVersion`, `dbAdminer` — Postgres 17 gets installed and running,
plus Adminer on port 3800 so you can browse the database in a browser.

The `vm:` block is Hyper-V settings — where the disks live, dynamic memory,
Secure Boot, and a checkpoint taken right after the install so you can roll
back to a clean machine.

Underneath it, `dataDisks`. We attach two extra disks, labelled E and L, and
they come up at `/mnt/e` and `/mnt/l`. We use Windows drive letters as labels
so it's familiar, but inside the VM you just `cd /mnt/e`. They're attached
after the install finishes, so the installer can't accidentally target one.

`bootstrap:` is the toolchain switchboard — install the desktop, Docker, ddev,
Node, Python, VS Code, Chrome, yes or no, and which VS Code extensions. Note
`vscodeExtensions` versus `extraVscodeExtensions`: setting the first one in
your local file *replaces* the team list. Use the second to add to it.

Then `git:` and `github:` — default branch, and how GitHub auth gets set up
inside the guest, including the gh CLI."

## 5. The ssh_keys directory  (~45s)

[ ssh_keys/ open, then its README ]

"`ssh_keys` is how credentials get in without editing any config.

Drop a private key in here and it's installed as the guest's SSH identity —
that's what makes the `git@github.com` clones work unattended on first boot.
Drop a `.pub` in and it goes into the guest's authorized_keys, so you can SSH
in from your laptop without a password. Typical setup is to copy in the key you
already use for GitHub, plus the public half.

Nothing in this directory is ever committed — there's a `.gitignore` in here
that ignores everything except itself and the README, on top of the root one.

If you drop nothing in, the guest generates its own key on first boot and
prints it, and you add it to GitHub yourself.

One honest caveat, and it's in the README: a private key you drop here is
written to the VM's seed disk in plain text on the way in. Bootstrap shreds the
staged copy afterward. If that's not acceptable for your key, leave this
directory empty and let the guest make its own."

## 6. Running it  (~60s)

[ elevated PowerShell at the repo root ]

"Everything goes through `vm.ps1`. Run it with no arguments and it prints what
it can do.

`.\vm.ps1 create` builds the VM from config — that's the long one.
`.\vm.ps1 open` starts it if it needs starting and opens the desktop.
`.\vm.ps1 start` and `.\vm.ps1 stop` are just power.
`.\vm.ps1 status` tells you what it is, where to reach it, how it's doing —
and `status -Report` prints the bootstrap log from inside the guest, which is
how you watch a build or work out why one failed.
`.\vm.ps1 remove` deletes the VM, and it won't touch the disks unless you add
`-DeleteDisks`.
`.\vm.ps1 hash` makes a password hash if you'd rather not have a plaintext
password anywhere.

Arguments pass straight through to the underlying script, so anything those
accept works here. And everything except `hash` needs an elevated PowerShell,
because the Hyper-V cmdlets do."

[ run `.\vm.ps1 create`, let the plan print ]

"Create prints its plan first — what it's about to build — then works through
seven or eight steps: check the name's free, resolve the ISO, build the seed
disk, make the VM, run the unattended install, checkpoint, finish. Then the
guest reboots into bootstrap and installs the toolchain on its own."

## 7. scripts/ — one line  (~15s)

[ scripts/ expanded, don't open anything ]

"`vm.ps1` is a thin router. The real work is in `scripts` — one script per
command, and a `lib` folder underneath for the config loader, the Hyper-V
calls, the ISO download and the seed disk builder. You don't need to go in
there to use this. You go in there when you want to change how it works."

## 8. Close  (~20s)

"So, to recap: blueprint says what OS — swap it and you get the same
environment somewhere else. Bootstrap installs the tools.
`vm.config.yaml` is what the team shares, `vm.config.local.yaml` is yours and
never gets committed. Drop keys in `ssh_keys`. Then `vm.ps1 create`, and go get
a coffee.

If a build fails, `.\vm.ps1 status -Report` first — it'll usually tell you
exactly which bootstrap step died."
