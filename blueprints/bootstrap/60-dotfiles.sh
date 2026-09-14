# Optional dotfiles checkout. Runs whatever the repo calls its installer, if
# any - otherwise it just leaves the clone in place for the user to wire up.
REPO="${VM_DOTFILES_REPO:-}"
if [[ -z "$REPO" ]]; then
    log "no dotfiles repo configured"
    return 0 2>/dev/null || exit 0
fi

log "cloning $REPO"
run_as_user "test -d \$HOME/.dotfiles && git -C \$HOME/.dotfiles pull --ff-only || git clone '${REPO}' \$HOME/.dotfiles"
run_as_user "for i in install install.sh bootstrap bootstrap.sh setup.sh; do if [ -x \$HOME/.dotfiles/\$i ]; then echo \"running .dotfiles/\$i\"; \$HOME/.dotfiles/\$i; break; fi; done"
