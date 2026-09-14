# VS Code from Microsoft's own repository, so it updates with the rest of the
# system rather than being a one-off download.
if ! is_true "${VM_INSTALL_VSCODE:-true}"; then
    log "vscode install disabled"
    return 0 2>/dev/null || exit 0
fi
if ! is_true "${VM_INSTALL_DESKTOP:-true}"; then
    log "vscode needs a desktop; skipping"
    return 0 2>/dev/null || exit 0
fi

# The package is called 'code' on both, so only the repo definition differs.
case "$PKG_MANAGER" in
    apt)
        add_apt_repo microsoft \
            'https://packages.microsoft.com/keys/microsoft.asc' \
            'deb [arch=amd64,arm64,armhf signed-by=/etc/apt/keyrings/microsoft.gpg] https://packages.microsoft.com/repos/code stable main'
        ;;
    dnf|yum)
        # dnf checks the signature against the rpm keyring rather than a keyring
        # file named in the repo line, so the key is imported separately.
        rpm --import https://packages.microsoft.com/keys/microsoft.asc 2>/dev/null || \
            warn "could not import the Microsoft signing key"

        add_dnf_repo vscode "[code]
name=Visual Studio Code
baseurl=https://packages.microsoft.com/yumrepos/vscode
enabled=1
autorefresh=1
type=rpm-md
gpgcheck=1
gpgkey=https://packages.microsoft.com/keys/microsoft.asc"
        ;;
    *) die "no supported package manager for vscode" ;;
esac

pkg_install code
code --version 2>/dev/null || true

# ------------------------------------------------------------------ extensions
#
# Installed as the developer, never as root: extensions live in
# ~/.vscode/extensions, and running `code` under sudo would put them in root's
# home where the user's editor will never look for them - and leave root-owned
# files scattered through ~/.vscode that break the next install.
#
# One at a time, and a failure is a warning rather than a build failure. A typo
# in an extension id, or one that has been unpublished, should cost you that
# extension and not the machine.
if [[ -z "${VM_VSCODE_EXTENSIONS:-}" ]]; then
    log "no vscode extensions requested"
else
    log "installing vscode extensions for $VM_PRIMARY_USER"
    ext_ok=0
    ext_failed=()
    for ext in $VM_VSCODE_EXTENSIONS; do
        if run_as_user "code --install-extension '$ext' --force" >/dev/null 2>&1; then
            log "  ok      $ext"
            ext_ok=$((ext_ok + 1))
        else
            log "  FAILED  $ext"
            ext_failed+=("$ext")
        fi
    done
    log "vscode extensions: $ext_ok installed, ${#ext_failed[@]} failed"
    if [[ ${#ext_failed[@]} -gt 0 ]]; then
        warn "could not install: ${ext_failed[*]}"
        warn "  check the id is publisher.name exactly as the Marketplace shows it"
        warn "  retry one by hand:  code --install-extension <id>"
    fi
fi
