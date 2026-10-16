# Extra virtual disks: partition-free filesystems, labelled, mounted, in fstab.
#
# Runs first so every data volume is in place before the rest of bootstrap
# writes anything.
#
# Reads /etc/vm-bootstrap.disks (tab separated):
#   label <TAB> scsi-lun <TAB> mountpoint <TAB> filesystem <TAB> sizeGB
DISKS_FILE=/etc/vm-bootstrap.disks

if [[ ! -s "$DISKS_FILE" ]] || ! grep -qvE '^\s*(#|$)' "$DISKS_FILE"; then
    log "no extra data disks configured"
    return 0 2>/dev/null || exit 0
fi

pkg_install rsync util-linux e2fsprogs
if grep -qP '\txfs\t' "$DISKS_FILE"; then pkg_install xfsprogs; fi

# Never, under any circumstances, touch the disk holding /.
root_source="$(findmnt -no SOURCE / | sed 's/\[.*\]//')"
root_parent="$(lsblk -no PKNAME "$root_source" 2>/dev/null | head -1 | tr -d ' ')"
if [[ -n "$root_parent" ]]; then
    ROOT_DISK="/dev/$root_parent"
else
    ROOT_DISK="$root_source"
fi
log "system disk is $ROOT_DISK (leaving it alone)"

# A disk we are allowed to format: a whole disk, not the system disk, with no
# partitions and no filesystem of its own. That last check is what keeps the
# CIDATA seed disk safe - it carries a vfat filesystem.
is_blank_disk() {
    local dev="$1"
    [[ -b "$dev" ]] || return 1
    [[ "$dev" != "$ROOT_DISK" ]] || return 1
    [[ "$(lsblk -dno TYPE "$dev" 2>/dev/null | tr -d ' ')" == "disk" ]] || return 1
    # More than one line means it has partitions.
    [[ "$(lsblk -no NAME "$dev" 2>/dev/null | wc -l)" -eq 1 ]] || return 1
    [[ -z "$(lsblk -dno FSTYPE "$dev" 2>/dev/null | tr -d ' ')" ]] || return 1
    return 0
}

disk_size_gb() {
    local dev="$1" bytes
    bytes="$(blockdev --getsize64 "$dev" 2>/dev/null || echo 0)"
    echo $(( bytes / 1024 / 1024 / 1024 ))
}

# Find the device for one configured disk. Preference order:
#   1. a filesystem already carrying this label - nothing to do, reuse it
#   2. the disk's serial or device id, which libvirt and vfkit set to the label
#   3. the SCSI LUN the host attached it at (deterministic)
#   4. the only blank disk of about the right size (belt and braces)
resolve_disk() {
    local label="$1" lun="$2" want_gb="$3"
    local existing candidate path found actual

    existing="$(blkid -L "$label" 2>/dev/null || true)"
    if [[ -n "$existing" ]]; then
        printf '%s\treuse\n' "$existing"
        return 0
    fi

    # The host names each disk after its label where the hypervisor allows it:
    # libvirt sets the disk's serial, which appears as
    # scsi-0QEMU_QEMU_HARDDISK_<label>, and vfkit sets a virtio-blk device id,
    # which appears as virtio-<label>. Hyper-V has no such field, so on Hyper-V
    # this finds nothing and the LUN below does the work.
    # Exact names, not a wildcard: a glob like *-data would also match a disk
    # labelled big-data.
    for path in /dev/disk/by-id/virtio-"${label}" \
                /dev/disk/by-id/scsi-0QEMU_QEMU_HARDDISK_"${label}"; do
        [[ -e "$path" ]] || continue
        candidate="$(readlink -f "$path")"
        if is_blank_disk "$candidate"; then
            printf '%s\tid\n' "$candidate"
            return 0
        fi
    done

    for path in /dev/disk/by-path/*scsi-0:0:0:"${lun}"; do
        [[ -e "$path" ]] || continue
        candidate="$(readlink -f "$path")"
        if is_blank_disk "$candidate"; then
            printf '%s\tlun\n' "$candidate"
            return 0
        fi
    done

    found=""
    for candidate in /dev/sd? /dev/vd?; do
        is_blank_disk "$candidate" || continue
        actual="$(disk_size_gb "$candidate")"
        # VHDX sizes round, so allow a little slack either way.
        if [[ "$actual" -ge $(( want_gb - 2 )) && "$actual" -le $(( want_gb + 2 )) ]]; then
            if [[ -n "$found" ]]; then
                warn "more than one blank ${want_gb} GB disk; cannot tell which is '$label'"
                return 1
            fi
            found="$candidate"
        fi
    done
    if [[ -n "$found" ]]; then
        printf '%s\tsize\n' "$found"
        return 0
    fi
    return 1
}

# Move whatever is already at the mount point onto the new volume, so pointing a
# disk at a directory that already has files in it does not hide them. Normally a
# no-op, since /mnt/<letter> is a fresh directory.
migrate_existing() {
    local dev="$1" mp="$2" staging
    [[ -d "$mp" ]] || return 0
    [[ -n "$(ls -A "$mp" 2>/dev/null)" ]] || return 0

    staging="$(mktemp -d)"
    log "migrating existing contents of $mp onto $dev"
    mount "$dev" "$staging"
    # The trailing /. copies the directory's own ownership and mode too, which
    # matters for /var/log (root:syslog) and anything with a non-default mode.
    rsync -aHAX --numeric-ids "$mp/." "$staging/"
    umount "$staging"
    rmdir "$staging"
}

while IFS=$'\t' read -r label lun mountpoint fstype size_gb; do
    [[ -n "${label:-}" ]] || continue
    [[ "${label:0:1}" == "#" ]] && continue

    if findmnt -rno SOURCE --target "$mountpoint" 2>/dev/null | grep -q .; then
        if [[ "$(findmnt -rno TARGET --target "$mountpoint")" == "$mountpoint" ]]; then
            log "$mountpoint is already a mount point; leaving it"
            continue
        fi
    fi

    resolved="$(resolve_disk "$label" "$lun" "$size_gb")" || {
        warn "could not find the '$label' disk (expected ${size_gb} GB at scsi lun $lun)"
        warn "current block devices:"
        lsblk -o NAME,SIZE,TYPE,FSTYPE,LABEL,MOUNTPOINT >&2
        continue
    }
    device="$(printf '%s' "$resolved" | cut -f1)"
    how="$(printf '%s' "$resolved" | cut -f2)"

    if [[ "$how" == "reuse" ]]; then
        log "disk '$label' already formatted at $device"
    else
        log "formatting $device as $fstype labelled '$label' (matched by $how)"
        # No partition table: the filesystem goes on the whole device. One less
        # thing to get wrong, and growing the disk later is just a resize.
        case "$fstype" in
            ext4) mkfs.ext4 -F -L "$label" "$device" ;;
            xfs)  mkfs.xfs -f -L "$label" "$device" ;;
            *)    warn "unsupported filesystem '$fstype' for '$label'"; continue ;;
        esac
        migrate_existing "$device" "$mountpoint"
    fi

    install -d "$mountpoint"
    # nofail: a detached or failed data disk must not leave the VM stuck in
    # emergency mode at boot.
    ensure_line "LABEL=${label} ${mountpoint} ${fstype} defaults,nofail 0 2" /etc/fstab

    systemctl daemon-reload
    if ! findmnt -rno TARGET "$mountpoint" >/dev/null 2>&1; then
        log "mounting $mountpoint"
        mount "$mountpoint"
    fi

    if [[ "$mountpoint" == "/var/log" ]]; then
        # The logging daemons are holding open files that just moved.
        systemctl restart systemd-journald 2>/dev/null || true
        systemctl restart rsyslog 2>/dev/null || true
    fi
done < "$DISKS_FILE"

log "disk layout:"
lsblk -o NAME,SIZE,TYPE,FSTYPE,LABEL,MOUNTPOINT
