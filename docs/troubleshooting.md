# Troubleshooting

## Build-time

### Which step failed?

The build prints a report before it exits, so start there rather than reading back
through the output:

```
  #   STEP                                     STATUS     TIME
  --- ---------------------------------------- ---------- -----
  3   Resolving install media                  ok         41s
  4   Building the seed disk                   FAILED     2s
  5-8 4 remaining step(s)                      not reached

  FAILED at: Building the seed disk
  New-VHD: there is not enough space on the disk.
```

Host steps depend on each other, so the build stops at the first failure instead
of carrying on. Nothing is rolled back: with `-All`, machines that finished before
the failure are left in place and named in the report, so re-run the rest with
`-Machine`. Add `-Verbose` for a stack trace.

A failure before step 1 (a bad blueprint name, a missing prerequisite) prints the
message on its own, since there is nothing yet to tabulate.

### "This script must run in an elevated PowerShell session"

The Hyper-V cmdlets require Administrator. Start PowerShell with "Run as
administrator". `-RenderOnly` is the one mode that works unelevated.

### "Virtual switch 'Default Switch' does not exist"

`Default Switch` is Hyper-V's built-in NAT switch. It is normally present on
Windows Pro, but it can be missing. Either point at an existing switch:

```powershell
VM = @{ SwitchName = 'My External Switch' }
```

or create one:

```powershell
New-VMSwitch -Name 'dev-nat' -SwitchType Internal
```

Every machine in a fleet must share one switch, or mDNS (`<host>.local`) will
not work between them.

### "openssl.exe was not found"

Password hashing uses the `openssl` bundled with Git for Windows. Install Git
for Windows, or generate a hash elsewhere and set
`Guest.PrimaryUser.PasswordHash` instead of `Password`.

### The installer stops at a prompt instead of running unattended

The seed disk was not picked up, so the installer fell back to interactive mode.
In order of likelihood:

1. **Check the seed disk is attached.** `Get-VMHardDiskDrive -VMName <name>`
   should list `<name>-seed.vhdx` alongside the system disk.
2. **Check the label.** In the installer, switch to a shell (Ctrl+Alt+F2) and run
   `lsblk -f`. One small vfat volume must carry exactly the right label, because
   that label is the whole contract:

   | Blueprint | Label | File the installer reads |
   | --- | --- | --- |
   | Ubuntu | `CIDATA` | `user-data` |
   | Fedora | `OEMDRV` | `ks.cfg` |

3. **The manual fallback**, which takes fifteen seconds. At the boot menu press
   `e` and add to the kernel line: `autoinstall` for Ubuntu, or
   `inst.ks=hd:LABEL=OEMDRV:/ks.cfg` for Fedora. Then Ctrl+X. If that is what
   unsticks it, say so: it means the file was found but auto-loading was not
   triggered.

Logs worth reading from the installer shell: `/var/log/cloud-init.log` on Ubuntu
(`grep -i nocloud`), `/tmp/anaconda.log` and `/tmp/program.log` on Fedora.

### The install times out

Default is 60 minutes (`Provisioning.InstallTimeoutMinutes`). A slow mirror or a
`updates: security` pass on a bad connection can exceed it. Open
`vmconnect.exe localhost <name>` and look before raising it - a hung installer
looks quite different from a slow one.

### "A VM named 'x' already exists"

`-Force` deletes and rebuilds it. Without `-Force` nothing is destroyed, which
is the intended default.

### The download fails or the checksum does not match

The ISO is verified against the vendor's published `SHA256SUMS`; a mismatched
file is deleted rather than used. Re-run to try again. BITS is used first and
resumes across interruptions. To use a file you already have:

```powershell
Iso = @{ Path = 'C:\ISO\ubuntu-24.04.3-live-server-amd64.iso' }
```

## First boot and bootstrap

### Where do the logs live?

Two files, written in the guest:

| Path | What is in it |
| --- | --- |
| `/var/log/vm-bootstrap-report.txt` | One page: every step, whether it passed, and how long it took. Start here. |
| `/var/log/vm-bootstrap.log` | Every line of output from the whole run. Go here once the report tells you which step to look at. |

The report is also the last thing printed to the console, and
`/etc/vm-bootstrap.summary` (which is the login banner) points at both files. So
someone handed the VM a week later can find out what happened without being told
where to look:

```bash
cat /var/log/vm-bootstrap-report.txt
```

```
 STEP                   STATUS   TIME
 ---------------------- -------- -----
 00-disks               ok       4s
 01-base                ok       96s
 32-docker              FAILED   11s
 33-ddev                ok       19s

 12 step(s): 11 ok, 1 failed, 0 skipped   total 383s

 FAILED: 32-docker
 Re-run just those:
     sudo vm-bootstrap --only 32-docker --force
```

It ends with a list of every log on the machine, including the installer's own
(`cloud-init-output.log` on Ubuntu, `anaconda.log` on Fedora) when the problem
predates bootstrap.

### How do I watch it live?

```bash
tail -f /var/log/vm-bootstrap.log       # in the guest
sudo vm-bootstrap --list                # per-step state
journalctl -u vm-bootstrap.service -f
```

From the host: `.\scripts\Get-DevVMStatus.ps1 -Follow`.

### A step failed

The runner deliberately continues past a failed step rather than leaving the
machine half-built and unusable. Fix the cause and re-run just that step:

```bash
sudo vm-bootstrap --only 32-docker --force
```

Steps that completed are stamped in `/var/lib/vm-bootstrap/*.done` and skipped;
`/var/lib/vm-bootstrap/complete` exists only after a clean full run.

### Bootstrap never started

```bash
systemctl status vm-bootstrap.service
cloud-init status --long
sudo cat /var/log/cloud-init-output.log
```

The unit has `ConditionPathExists=!/var/lib/vm-bootstrap/complete`, so it is
skipped once a clean run has happened - that is not a failure.

### It rebooted and I lost the console

Expected: `99-finalize.sh` reboots into GDM. Set
`Bootstrap.RebootWhenDone = $false` to stay put.

## Desktop and display

### The desktop is 1024x768 and will not resize

A basic session cannot be resized from inside the guest. Either use Enhanced
Session (below), or set `Bootstrap.DisplayResolution` and re-run:

```bash
sudo vm-bootstrap --only 10-desktop --force && sudo reboot
```

### Enhanced Session does not connect

It needs all of: `VM.EnhancedSession = $true` (sets the host transport to
`HvSocket`), xrdp listening on a vsock in the guest, and an Xorg session.

```powershell
Get-VM fedora-dev | Select-Object Name, EnhancedSessionTransportType
```

```bash
systemctl status xrdp
grep -E '^port=' /etc/xrdp/xrdp.ini          # expect port=vsock://-1:3389
grep WaylandEnable /etc/gdm3/custom.conf     # expect WaylandEnable=false
sudo vm-bootstrap --only 20-hyperv --force
```

Then close and reopen the VM window - Hyper-V decides basic vs enhanced at
connect time. Log in at the xrdp prompt, not the GDM greeter.

### A black screen or "authentication required" pop-ups over xrdp

Both are Wayland or polkit issues that `20-hyperv.sh` handles; if they appear,
that step probably failed. Re-run it as above. GNOME must be on Xorg - xrdp
cannot attach to a Wayland session.

## Disks

### A data disk did not appear

`00-disks.sh` logs the full block-device list when it cannot match a disk. From
the guest:

```bash
lsblk -o NAME,SIZE,TYPE,FSTYPE,LABEL,MOUNTPOINT
ls -l /dev/disk/by-path/ | grep scsi
cat /etc/vm-bootstrap.disks
sudo vm-bootstrap --only 00-disks --force
```

From the host, check the disk is attached at the location the guest expects
(`/etc/vm-bootstrap.disks` lists the LUN per label):

```powershell
Get-VMHardDiskDrive -VMName fedora-dev |
    Format-Table ControllerNumber, ControllerLocation, Path
```

Expected: `0:0` system, `0:2` seed, `0:3`+ data disks.

### A directory looks empty after mounting a disk over it

Only relevant if you pointed a `MountPoint` at a directory that already had files
in it. The step migrates existing contents onto the new volume before mounting, so
this should not happen; if it did, the originals are still on the system disk,
hidden under the mount:

```bash
sudo mkdir -p /mnt/root-disk
sudo mount -o bind / /mnt/root-disk      # a bind mount shows what is underneath
ls /mnt/root-disk/mnt/e
```

The default layout mounts at `/mnt/e` and `/mnt/l`, which are fresh empty
directories, so there is nothing to migrate.

### A disk is full

```bash
df -h / /mnt/e /mnt/l
sudo du -xh --max-depth=1 / | sort -h | tail
```

The system disk is 25 GB by default and Docker images live on it, in
`/var/lib/docker`. Raise `DiskGB`, or give Docker its own volume: see
[disks.md](disks.md#a-note-on-the-25-gb-system-disk).

Growing one is a `Resize-VHD` plus `resize2fs` - see
[disks.md](disks.md#growing-a-disk). There is no partition table in the way.

### The VM will not boot after removing a disk

It should: every data disk is mounted `nofail` precisely so a missing one cannot
drop the machine into emergency mode. If it does hang, the fstab entry was
probably hand-edited. Boot the recovery console and remove the line.

## ddev

```bash
ddev version
docker ps                        # must work without sudo
groups                           # must include docker
sudo vm-bootstrap --only 33-ddev --force
```

**`docker: permission denied`** - the docker group membership needs a fresh
login. Log out and back in, or `newgrp docker`.

**Browser warns the ddev site is untrusted** - the mkcert CA was not installed
for your user. `mkcert -install` then restart the browser. It must run as the
developer, not root.

## Repository clones

### A repo did not clone

```bash
sudo vm-bootstrap --only 70-repos --force    # retry; already-cloned repos are just fetched
cat /etc/vm-bootstrap.repos                  # what the guest was told to clone
sudo -iu $USER ssh -T git@github.com         # is the SSH key accepted
gh auth status                               # is https auth working
```

Almost always this is a private repo whose remote is `git@github.com:` while the
SSH key was generated inside the guest and never registered on the account. The
build warns about that combination up front. Fixes, and the unattended option,
are in [repos.md](repos.md#private-repositories).

### The report says a step failed but everything looks installed

If `70-repos` is the failed step, that is by design - a repo you asked for is
missing, so the machine is not finished. `/var/lib/vm-bootstrap/complete` is
deliberately not written, which is why `Get-DevVMStatus.ps1` reports
`finished with failed step(s)` rather than complete. Everything that did clone
stayed cloned.

From the host, that same script will fetch the guest's report for you:

```powershell
.\scripts\Get-DevVMStatus.ps1 -Report
```

### A clone hung

It should not - `GIT_TERMINAL_PROMPT=0` turns a missing credential into an
immediate failure rather than a username prompt that would block bootstrap
forever. A genuine hang means the remote is unreachable; check DNS and any
corporate proxy.

### Will re-running overwrite my local work?

No. An existing clone is only ever `git fetch`ed - never reset, never pulled, no
merge. A path that exists but is not a git repository is skipped with a warning.

## Databases

### The server installed but nothing can connect

Work through it in this order, because each layer hides the one below it.

```bash
sudo systemctl status postgresql       # or mysql / mysqld
sudo ss -lntp | grep -E '5432|3306'    # is it listening, and on which address?
```

`127.0.0.1:5432` means it is bound to localhost only. Set
`Database.ListenAddress` to `'*'` and re-run `sudo vm-bootstrap --only 35-database
--force`.

If it is listening on `0.0.0.0` but a remote client still cannot connect, the
firewall is next. On Fedora only ssh and mDNS are open by default:

```bash
sudo firewall-cmd --list-ports
sudo firewall-cmd --permanent --add-port=5432/tcp && sudo firewall-cmd --reload
```

If the connection reaches the server and is refused, it is authentication.
Postgres needs a `pg_hba.conf` rule for the client, MySQL needs the user to exist
with a host of `'%'`. Both are added automatically when `ListenAddress` is `'*'`.

### The server will not start at all, on a non-default port

SELinux, almost certainly. It labels ports as well as files, so Postgres on 5433
is denied the bind and the failure looks like the daemon crashing:

```bash
sudo semanage port -l | grep postgresql_port_t
sudo semanage port -a -t postgresql_port_t -p tcp 5433
```

`35-database.sh` does this automatically, so hitting it by hand means either the
port was changed afterwards or `policycoreutils-python-utils` could not be
installed. The bootstrap report says which.

### A browser cannot open the database

It never will. Postgres and MySQL speak their own wire protocols, not HTTP, so
`http://host.local:5432` fails and the server logs something like `invalid startup
packet length`. That is the wrong client, not a broken network. Use `psql` or
`mysql`, or `Test-NetConnection host.local -Port 5432` from Windows to test only
whether the port is reachable.

### I asked for MySQL 8.4 and got 8.0

Expected, and reported rather than silent. MySQL comes from the distribution
because there is no MySQL repository that installs unattended on both distros, so
the version is whatever that release ships. Pin a version your distribution
carries, or use Postgres, which is installed from the upstream repository and can
give you an exact major version on either OS. See
[databases.md](databases.md).

### `.local` does not resolve from inside a container

Expected. mDNS resolution comes from avahi and nss-mdns on the host OS, and a
Docker container has neither. Use the IP or a Docker network alias rather than the
`.local` name.

## Adminer

### The route does not load

```bash
systemctl status adminer.service
journalctl -u adminer.service
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3800/
```

A 200 from inside the guest but nothing from your laptop means the firewall.
`36-adminer.sh` opens the port, so hitting this by hand suggests the port was
changed afterwards:

```bash
sudo firewall-cmd --permanent --add-port=3800/tcp && sudo firewall-cmd --reload
```

### A blank page instead of Adminer

Usually the downloaded file was an error page rather than Adminer. The step runs
`php -l` on it for that reason and refuses to serve something that cannot parse,
so a blank page more often means a missing PHP extension:

```bash
php -m | grep -iE 'pgsql|mysql'
```

If the driver is absent, Adminer loads but cannot offer your engine. Re-run the
step, which installs the driver matching `Database.Engine`:

```bash
sudo vm-bootstrap --only 36-adminer --force
```

## Files and hooks

### Files did not appear

```bash
cat /etc/vm-bootstrap.files          # what the seed said to copy, and where
ls -la /var/lib/vm-bootstrap/files/  # the staged trees
sudo vm-bootstrap --only 06-files --force
```

The manifest is tab separated with a literal `-` where no explicit mode was given.
That placeholder matters: tab is whitespace to the shell, so `read` collapses a run
of tabs and an empty field would silently shift every field after it.

### A post-start hook failed

The generated script runs with `set -euo pipefail`, so it stops at the first
failure and the step is reported failed. Read what it printed, fix the commands in
`Hooks.PostStart`, then re-run:

```bash
cat /var/log/vm-post-start.log
sudo vm-bootstrap --only 80-hooks --force
```

The script itself is at `/usr/local/lib/vm-bootstrap/hooks/post-start.sh` and can
be run directly while you work out what is wrong. `-RenderOnly` also writes it to
`build/<vm>/post-start.sh`, so you can read exactly what will run before building.

### A hook cannot find nvm, node or ddev

`RunAs = 'user'` runs through a login shell for exactly this reason, so the user's
`PATH` is set up as if they had logged in. `RunAs = 'root'` does not: root has none
of the primary user's shell configuration. If a hook needs both, keep it as `user`
and use `sudo` inside the hook for the parts that need it.

### Hooks ran twice, or keep running

`Hooks.RunAtEveryBoot` installs `vm-post-start.service`, which runs on every start
by design. Commands under it need to be idempotent.

```bash
systemctl disable vm-post-start.service   # stop it running again
```

## Networking and mDNS

### `ping other-vm.local` fails

```bash
systemctl status avahi-daemon               # must be running on BOTH machines
avahi-browse -at                            # who is advertising
hostnamectl --static                        # the name being published
getent hosts other-vm.local                 # is nsswitch wired up
grep mdns /etc/nsswitch.conf                # expect mdns4_minimal on hosts:
```

Then check the things mDNS cannot fix:

- **Both VMs must be on the same virtual switch.** Multicast does not cross
  switches. `Get-VMNetworkAdapter -VMName *` shows what each is attached to.
- **A firewall may be dropping UDP 5353.** `15-mdns.sh` opens it if ufw is
  active, but a rule added later can close it again.
- **Only one machine may claim a name.** Avahi renames a collision to
  `host-2.local`; `hostnamectl --static` reveals it.

### The guest has no network at all

Confirm the switch exists and the adapter is connected:

```powershell
Get-VMNetworkAdapter -VMName fedora-dev | Format-List Name, SwitchName, Status, IPAddresses
```

The `Default Switch` NAT network hands out addresses over DHCP and its subnet
changes across host reboots - which is exactly why the tooling addresses guests
by `<hostname>.local` rather than by IP.

### `Get-DevVMStatus.ps1` says "no ssh access yet"

It probes over SSH with `BatchMode=yes`, so it needs key auth. Add your public
key to `Guest.PrimaryUser.AuthorizedKeys` and rebuild, or copy it up:

```bash
ssh-copy-id dev@acme-fedora-workstation.local
```

Everything the host can see is reported regardless: state, uptime, address,
blueprint, CPU, memory, and every attached disk with its allocated size and what
it currently costs on the host. What needs the guest is the OS release actually
running, the kernel, real disk usage, the account list, and bootstrap state.

## Stopping and starting

### It will not shut down

`Stop-DevVM.ps1` polls for two minutes and then tells you the guest is ignoring
the request. That usually means a service is hung or something is holding a
filesystem. Look first, since the console often shows exactly what is blocking:

```powershell
vmconnect.exe localhost acme-fedora-workstation
```

Then force it, accepting the same risk as pulling the plug on a real machine:

```powershell
.\scripts\Stop-DevVM.ps1 -TurnOff
```

If graceful shutdown never works, the integration services may not be running in
the guest:

```bash
systemctl status hv-kvp-daemon      # or hypervkvpd on Fedora
sudo vm-bootstrap --only 20-hyperv --force
```

### It started but nothing answers

`Start-DevVM.ps1 -Wait` polls for an address and reports if none arrives. mDNS
takes a few seconds longer than the address does, so `<host>.local` failing
immediately after boot is normal. Check what state it reached:

```powershell
.\scripts\Get-DevVMStatus.ps1
```

### Did starting it re-run provisioning and overwrite my work?

No. `vm-bootstrap.service` has
`ConditionPathExists=!/var/lib/vm-bootstrap/complete`, so once a clean run has
happened the unit is skipped on every subsequent boot. Confirm from inside:

```bash
ls -l /var/lib/vm-bootstrap/complete
systemctl status vm-bootstrap.service     # expect "condition failed"
```

Only `New-DevVM.ps1 -Force` rebuilds a machine, and only
`Remove-DevVM.ps1 -DeleteDisks` deletes disks.

### The disks still take up space after shutting down

Expected. Stopping releases CPU and RAM, not disk. The VHDX files are dynamically
expanding, so they cost what was written to them, and deleting files inside the
guest does not shrink the file on its own. With the VM off:

```powershell
Optimize-VHD -Path C:\VMs\acme-fedora-workstation\acme-fedora-workstation.vhdx -Mode Full
```

## Recovering

A checkpoint named `clean-install (pre-bootstrap)` is taken after the OS install
and before any tooling, so a bad bootstrap does not cost you the OS install:

```powershell
Get-VMSnapshot -VMName fedora-dev
Restore-VMSnapshot -VMName fedora-dev -Name 'clean-install (pre-bootstrap)' -Confirm:$false
Start-VM fedora-dev
```

To start over completely:

```powershell
.\scripts\Remove-DevVM.ps1 -Machine ubuntu-gui -DeleteDisks
.\scripts\New-DevVM.ps1 -Machine ubuntu-gui
```

`Remove-DevVM.ps1` leaves the VM directory alone if it still contains files it
does not recognise, so it will not take your data with it.
