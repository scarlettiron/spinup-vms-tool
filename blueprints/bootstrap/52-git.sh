# Scarlett A. Scott
# Created 2026-09-15

# Global git configuration for the primary user.
set_git() {
    local key="$1" value="$2"
    [[ -n "$value" ]] || return 0
    log "git config --global $key"
    run_as_user "git config --global $(printf '%q' "$key") $(printf '%q' "$value")"
}

set_git user.name          "${VM_GIT_USER_NAME:-}"
set_git user.email         "${VM_GIT_USER_EMAIL:-}"
set_git init.defaultBranch "${VM_GIT_DEFAULT_BRANCH:-}"
set_git core.editor        "${VM_GIT_EDITOR:-}"

# Read by hub, some editor integrations, and handy in `git config --list` when
# you are working out which account a VM is talking to.
set_git github.user        "${VM_GITHUB_USERNAME:-}"

if is_true "${VM_GIT_PULL_REBASE:-false}"; then
    set_git pull.rebase true
fi

# Arbitrary extra pairs from Git.Extra.
EXTRA_FILE=/etc/vm-bootstrap.gitconfig-extra
if [[ -s "$EXTRA_FILE" ]]; then
    while IFS=$'\t' read -r key value; do
        [[ -n "${key:-}" ]] || continue
        [[ "${key:0:1}" == "#" ]] && continue
        set_git "$key" "${value:-}"
    done < "$EXTRA_FILE"
fi

if [[ -z "${VM_GIT_USER_EMAIL:-}" ]]; then
    warn "no Git.UserEmail configured - commits from this VM will prompt for an identity"
fi

run_as_user "git config --global --list" || true
