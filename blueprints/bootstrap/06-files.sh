# Move the files that were copied in from the host into place.
#
# The seed staged each directory under /var/lib/vm-bootstrap/files/<n>/ with its
# layout intact. This step moves each tree to its destination and sets ownership.
#
# Staging rather than writing straight to the destination is what lets a
# destination be '~': where a home directory actually is, and which uid owns it,
# are things only the guest knows. Runs after 05-users so any additional account
# named as an owner already exists.
MANIFEST=/etc/vm-bootstrap.files

if [[ ! -s "$MANIFEST" ]] || ! grep -qvE '^\s*(#|$)' "$MANIFEST"; then
    log "no files to copy in"
    return 0 2>/dev/null || exit 0
fi

copied=0
failed=0

while IFS=$'\t' read -r stage destination owner mode count; do
    [[ -n "${stage:-}" && "${stage:0:1}" != "#" ]] || continue
    # '-' means no explicit mode: the seed writes a placeholder rather than an
    # empty field, because tab is whitespace to `read` and a run of tabs collapses,
    # which would shift every field after the empty one.
    [[ "${mode:-}" == "-" ]] && mode=''

    if [[ ! -d "$stage" ]]; then
        warn "staged directory $stage is missing; nothing to copy for $destination"
        failed=$((failed + 1))
        continue
    fi

    # '~' and '~/...' mean the home of the owning user, which getent knows and
    # the host does not.
    target="$destination"
    owner_user="${owner%%:*}"
    if [[ "$target" == "~" || "$target" == "~/"* ]]; then
        home="$(getent passwd "$owner_user" | cut -d: -f6)"
        if [[ -z "$home" ]]; then
            warn "cannot resolve the home directory of '$owner_user'; skipping $destination"
            failed=$((failed + 1))
            continue
        fi
        if [[ "$target" == "~" ]]; then
            target="$home"
        else
            target="$home/${target#~/}"
        fi
    fi

    log "copying $count file(s) into $target (owner $owner)"
    install -d "$target"

    # -a preserves the modes the seed set, and the trailing /. copies the contents
    # rather than nesting the staging directory inside the destination.
    if ! cp -a "$stage/." "$target/"; then
        warn "could not copy into $target"
        failed=$((failed + 1))
        continue
    fi

    # Only the files that came from the staging tree are re-owned, so copying into
    # a shared destination such as /etc does not take ownership of the whole thing.
    while IFS= read -r relative; do
        [[ -n "$relative" ]] || continue
        item="$target/$relative"
        [[ -e "$item" ]] || continue
        chown "$owner" "$item" 2>/dev/null \
            || warn "could not chown $item to $owner"
        [[ -n "${mode:-}" ]] && chmod "$mode" "$item"
    done < <(cd "$stage" && find . -mindepth 1 -printf '%P\n')

    selinux_restore "$target"
    copied=$((copied + count))
done < "$MANIFEST"

log "copied $copied file(s) in"

if [[ $failed -gt 0 ]]; then
    warn "$failed director(ies) could not be copied"
    return 1 2>/dev/null || exit 1
fi
