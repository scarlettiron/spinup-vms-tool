# Google Chrome, from Google's own repository.
#
# Chrome rather than Chromium on purpose: it is what the team tests against, and
# the vendor repo means it updates with the rest of the system afterwards.
#
# Only the repository setup differs between distros, so this stays one shared step
# rather than two per-blueprint copies.
if ! is_true "${VM_INSTALL_CHROME:-true}"; then
    log "chrome install disabled"
    return 0 2>/dev/null || exit 0
fi

# A browser with no desktop to show it in is just a large download. The server
# blueprint turns the desktop off, so follow that rather than needing a second
# switch set to match.
if ! is_true "${VM_INSTALL_DESKTOP:-true}"; then
    log "no desktop on this machine, so skipping chrome"
    return 0 2>/dev/null || exit 0
fi

if have_cmd google-chrome || have_cmd google-chrome-stable; then
    log "chrome already installed: $(google-chrome --version 2>/dev/null || google-chrome-stable --version 2>/dev/null)"
    return 0 2>/dev/null || exit 0
fi

case "$PKG_MANAGER" in
    apt)
        add_apt_repo google-chrome \
            'https://dl.google.com/linux/linux_signing_key.pub' \
            'deb [arch=amd64 signed-by=/etc/apt/keyrings/google-chrome.gpg] https://dl.google.com/linux/chrome/deb/ stable main'
        pkg_install google-chrome-stable
        ;;
    dnf|yum)
        # Installing the package pulls in google-chrome.repo itself, but writing it
        # first keeps the source explicit and means the repo is present for
        # upgrades even if this install is interrupted.
        add_dnf_repo google-chrome "$(cat <<'REPO'
[google-chrome]
name=google-chrome
baseurl=https://dl.google.com/linux/chrome/rpm/stable/x86_64
enabled=1
gpgcheck=1
gpgkey=https://dl.google.com/linux/linux_signing_key.pub
REPO
)"
        pkg_install google-chrome-stable
        ;;
    *)
        warn "no supported package manager for chrome"
        return 1 2>/dev/null || exit 1
        ;;
esac

if have_cmd google-chrome || have_cmd google-chrome-stable; then
    log "installed $(google-chrome --version 2>/dev/null || google-chrome-stable --version 2>/dev/null)"
else
    warn "google-chrome-stable installed but no google-chrome binary is on the path"
    return 1 2>/dev/null || exit 1
fi
