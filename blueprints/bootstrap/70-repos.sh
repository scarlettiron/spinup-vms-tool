# Scarlett A. Scott
# Created 2026-09-15

# Clone the repositories listed in Repos.Items.
#
# Runs after 55-github.sh so the SSH key, known_hosts, and gh credential helper
# are already in place - which is what lets a private git@github.com: remote
# clone unattended.
#
# Reads /etc/vm-bootstrap.repos (tab separated):
#   url <TAB> directory <TAB> branch <TAB> depth <TAB> recurse(0|1) <TAB> base64(post-clone)
REPOS_FILE=/etc/vm-bootstrap.repos

if [[ ! -s "$REPOS_FILE" ]] || ! grep -qvE '^\s*(#|$)' "$REPOS_FILE"; then
    log "no repositories configured"
    return 0 2>/dev/null || exit 0
fi

USER_HOME="$(getent passwd "$VM_PRIMARY_USER" | cut -d: -f6)"
ROOT="${VM_REPOS_ROOT:-src}"
# A relative Root is relative to the developer's home.
case "$ROOT" in
    /*) ;;
    *) ROOT="$USER_HOME/$ROOT" ;;
esac

install -d -o "$VM_PRIMARY_USER" -g "$VM_PRIMARY_USER" "$ROOT"
log "cloning into $ROOT"

failed=()

while IFS=$'\t' read -r url directory branch depth recurse post_b64; do
    [[ -n "${url:-}" ]] || continue
    [[ "${url:0:1}" == "#" ]] && continue

    target="$ROOT/$directory"

    if [[ -d "$target/.git" ]]; then
        # Never reset or pull: uncommitted work in a checkout the developer has
        # already touched is not ours to throw away. Fetch so refs are current
        # and leave the working tree exactly as it is.
        log "$directory already cloned; fetching"
        if ! run_as_user "git -C $(printf '%q' "$target") fetch --all --prune --tags"; then
            warn "could not fetch $directory"
        fi
        continue
    fi

    if [[ -e "$target" ]]; then
        warn "$target exists but is not a git repository; skipping $url"
        failed+=("$directory")
        continue
    fi

    args="--recurse-submodules"
    [[ "${recurse:-0}" == "1" ]] || args=""
    [[ -n "${branch:-}" && "$branch" != "-" ]] && args="$args --branch $(printf '%q' "$branch")"
    [[ -n "${depth:-}" && "$depth" != "0" && "$depth" != "-" ]] && args="$args --depth $(printf '%q' "$depth")"

    install -d -o "$VM_PRIMARY_USER" -g "$VM_PRIMARY_USER" "$(dirname "$target")"

    log "cloning $url into $directory"
    # GIT_TERMINAL_PROMPT=0 turns a missing credential into an immediate failure
    # instead of a prompt that would hang bootstrap forever.
    if run_as_user "GIT_TERMINAL_PROMPT=0 git clone $args $(printf '%q' "$url") $(printf '%q' "$target")"; then
        if [[ -n "${post_b64:-}" && "$post_b64" != "-" ]]; then
            command="$(printf '%s' "$post_b64" | base64 -d)"
            log "running post-clone command in $directory"
            if ! run_as_user "cd $(printf '%q' "$target") && ${command}"; then
                warn "post-clone command failed for $directory"
                failed+=("$directory (post-clone)")
            fi
        fi
    else
        warn "clone failed: $url"
        warn "  if this is a private repo, check the SSH key is registered on the account:"
        warn "  sudo -iu $VM_PRIMARY_USER ssh -T git@github.com"
        failed+=("$directory")
    fi
done < "$REPOS_FILE"

if [[ ${#failed[@]} -gt 0 ]]; then
    warn "these repositories did not clone cleanly: ${failed[*]}"
    warn "fix access, then: sudo vm-bootstrap --only 70-repos --force"
    # Non-zero so the runner leaves this step un-stamped and a re-run retries it.
    # Everything that did clone stays cloned.
    exit 1
fi

log "repositories under $ROOT:"
ls -1 "$ROOT"
