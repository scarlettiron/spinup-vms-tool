# Disks

Each VM gets a system disk plus any number of extra data disks, declared in
`VM.DataDisks`. They are named by the drive letters production uses, because that
is how the team talks about them.

| Drive | Size | Config | Mounted at |
| --- | --- | --- | --- |
| `C:` | 25 GB | `DiskGB` | `/` (the system disk) |
| `E:` | 50 GB | `DataDisks` label `drive-e` | `/mnt/e` |
| `L:` | 25 GB | `DataDisks` label `drive-l` | `/mnt/l` |

```powershell
DiskGB = 25
VM = @{
    DataDisks = @(
        @{ Label = 'drive-e'; DriveLetter = 'E'; SizeGB = 50 }
        @{ Label = 'drive-l'; DriveLetter = 'L'; SizeGB = 25 }
    )
}
```

Every disk is **dynamically expanding**, so a 50 GB drive costs only what is
actually written to it. The three above total 100 GB of addressable space for a
few GB of real files.

## Mount points

Linux has no drive letters, so a filesystem still has to be attached somewhere.
Give a `DriveLetter` and the disk is mounted at `/mnt/<letter>`.

This deliberately does **not** take over a system directory. Production has no
mount points, so nothing here assumes `/var/log` or `/opt` is a separate volume:
a path that exists on the VM but not in production is a difference waiting to
bite. `/mnt/e` and `/mnt/l` are new, empty directories that mirror the
production naming and nothing else.

To attach a disk somewhere specific anyway, set `MountPoint` and it wins:

```powershell
@{ Label = 'drive-e'; DriveLetter = 'E'; SizeGB = 50; MountPoint = '/srv/data' }
```

An entry needs either a `DriveLetter` or a `MountPoint`. `C` is rejected as a
drive letter, since that is the system disk (use `DiskGB` for its size).

## How a disk finds its way to the right place

Host side ([HyperV.psm1](../scripts/lib/HyperV.psm1)): a VHDX per disk, attached
at a **fixed SCSI controller location**:

| Location | Device |
| --- | --- |
| 0:0 | system disk (`C:`) |
| 0:1 | install DVD (ejected after install) |
| 0:2 | CIDATA seed disk |
| 0:3 and up | data disks, in `DataDisks` order |

Guest side ([00-disks.sh](../blueprints/bootstrap/00-disks.sh)):
Hyper-V's synthetic SCSI controller reports the controller location as the LUN,
so `E:` is reliably `/dev/disk/by-path/...scsi-0:0:0:3` rather than a `/dev/sdc`
that could shuffle between boots. The step tries three things in order:

1. **a filesystem already carrying the label**, so nothing to do, reuse it
2. **the expected SCSI LUN**
3. **the only blank disk of about the right size**, a fallback in case by-path
   naming ever changes

Then it formats, labels, and mounts. The `fstab` entry is by label
(`LABEL=drive-e /mnt/e ext4 defaults,nofail 0 2`), so it survives device
renaming.

Locations are assigned in one place, `Get-DevVMDiskLocations` in
[Common.psm1](../scripts/lib/Common.psm1), which both the host and the seed
generator read, so they cannot drift apart.

## Two details worth knowing

**No partition tables.** The filesystem goes directly on the whole device
(`mkfs.ext4 -L drive-e /dev/sdc`). This is a normal pattern for data volumes, and
it means growing a disk later is one command with nothing to align. It is also
what makes "does this disk have a filesystem?" a reliable safety check, which is
how the step knows never to touch the CIDATA seed disk, which carries vfat.

**Existing content is migrated, not hidden.** Mounting a volume over a directory
that already has files in it would make them invisible. The step mounts the new
volume somewhere temporary first, copies anything already at the mount point
across with `rsync` (including the directory's own ownership and mode), unmounts,
and only then mounts it in place. With `/mnt/e` and `/mnt/l` there is nothing to
migrate, but the safeguard matters the moment somebody points a `MountPoint` at a
directory that is already populated.

The system disk is identified with `findmnt /` and excluded explicitly. A disk
with any partition or any filesystem is never formatted.

## Changing the layout

Edit `DataDisks` and rebuild, or for an existing VM add the disk on the host and
re-run the step:

```powershell
$vm = 'fedora-dev'
New-VHD -Path "C:\VMs\$vm\$vm-drive-f.vhdx" -SizeBytes 100GB -Dynamic
Add-VMHardDiskDrive -VMName $vm -Path "C:\VMs\$vm\$vm-drive-f.vhdx" -ControllerNumber 0 -ControllerLocation 5
```

```bash
sudo vm-bootstrap --only 00-disks --force
```

A disk whose label already exists is left alone, so re-running is safe.

### Growing a disk

```powershell
Stop-VM fedora-dev
Resize-VHD -Path C:\VMs\fedora-dev\fedora-dev-drive-e.vhdx -SizeBytes 100GB
Start-VM fedora-dev
```

```bash
sudo resize2fs /dev/disk/by-label/drive-e     # ext4
sudo xfs_growfs /mnt/e                        # xfs
```

No partition table means no partition to grow first.

### Dropping a disk

Remove it from `DataDisks`, then in the guest delete its `/etc/fstab` line before
detaching the VHDX. `nofail` means forgetting this leaves the VM bootable
regardless: it just logs a failed mount.

## A note on the 25 GB system disk

25 GB is enough for Ubuntu with a full GNOME desktop and the toolchain, but it is
not roomy. What fills a dev box is usually Docker images and ddev containers,
which live in `/var/lib/docker` on the system disk. If a build runs out of space
there, the options are to raise `DiskGB` or to give Docker its own volume:

```powershell
@{ Label = 'drive-e'; DriveLetter = 'E'; SizeGB = 50; MountPoint = '/var/lib/docker' }
```

The ordering already works out, since `00-disks.sh` runs before `32-docker.sh`,
so the volume is mounted before Docker writes anything. That does introduce a
mount point production does not have, which is the trade-off to weigh.

## Constraints the tooling enforces

- Labels: 16 characters maximum (an ext4 limit), letters, digits, `-` and `_`,
  and unique
- Drive letters: a single letter, not `C`, and unique
- Mount points: absolute, unique, and never `/`
- Filesystems: `ext4` or `xfs`
- Every entry needs a `Label`, a `SizeGB`, and either a `DriveLetter` or a
  `MountPoint`

Each is checked at build time by `Get-DevVMDataDiskLayout`, so a bad entry fails
during `-RenderOnly` rather than halfway through a first boot.
