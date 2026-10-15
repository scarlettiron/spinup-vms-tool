# Google Chrome, from Google's own repository.
#
# Chrome rather than Chromium on purpose: it is what the team tests against, and
# the vendor repo means it updates with the rest of the system afterwards.
#
# Except on arm64: Google publishes no Chrome for Linux on arm64, so an arm64
# guest (every guest on an Apple Silicon Mac) gets Chromium instead. Which one
# was installed is written to /var/lib/vm-bootstrap/browser - chrome, chromium
# or none - so the status command can say so.
#
# Only the repository setup differs between distros, so this stays one shared step
# rather than two per-blueprint copies.
BROWSER_FILE=/var/lib/vm-bootstrap/browser
record_browser() {
    install -d -m 0755 /var/lib/vm-bootstrap
    printf '%s\n' "$1" > "$BROWSER_FILE"
}

if ! is_true "${VM_INSTALL_CHROME:-true}"; then
    log "chrome install disabled"
    record_browser none
    return 0 2>/dev/null || exit 0
fi

# A browser with no desktop to show it in is just a large download. The server
# blueprint turns the desktop off, so follow that rather than needing a second
# switch set to match.
if ! is_true "${VM_INSTALL_DESKTOP:-true}"; then
    log "no desktop on this machine, so skipping chrome"
    record_browser none
    return 0 2>/dev/null || exit 0
fi

# ------------------------------------------------------------- arm64: chromium
if [[ "$GUEST_ARCH" == "aarch64" ]]; then
    log "there is no Google Chrome for Linux on arm64, so installing Chromium"
    if have_cmd chromium || have_cmd chromium-browser; then
        log "chromium already installed"
        record_browser chromium
        return 0 2>/dev/null || exit 0
    fi
    case "$PKG_MANAGER" in
        apt)
            # Ubuntu's chromium-browser .deb is only a wrapper that installs the
            # snap, so go to the snap directly when snapd is there. Debian
            # proper still packages it as 'chromium'.
            if [[ "$OS_ID" == "ubuntu" ]]; then
                if have_cmd snap && snap install chromium; then
                    record_browser chromium
                else
                    warn "could not install the chromium snap; no browser installed"
                    record_browser none
                fi
            elif pkg_install chromium; then
                record_browser chromium
            else
                warn "could not install chromium; no browser installed"
                record_browser none
            fi
            ;;
        dnf|yum)
            # Fedora packages it; on the RHEL family it comes from EPEL, which
            # 01-base.sh has already enabled.
            if pkg_install chromium; then
                record_browser chromium
            else
                warn "could not install chromium; no browser installed"
                record_browser none
            fi
            ;;
        *)
            warn "no supported package manager for chromium"
            record_browser none
            ;;
    esac
    return 0 2>/dev/null || exit 0
fi

if have_cmd google-chrome || have_cmd google-chrome-stable; then
    log "chrome already installed: $(google-chrome --version 2>/dev/null || google-chrome-stable --version 2>/dev/null)"
    record_browser chrome
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
        record_browser none
        return 1 2>/dev/null || exit 1
        ;;
esac

if have_cmd google-chrome || have_cmd google-chrome-stable; then
    log "installed $(google-chrome --version 2>/dev/null || google-chrome-stable --version 2>/dev/null)"
    record_browser chrome
else
    warn "google-chrome-stable installed but no google-chrome binary is on the path"
    record_browser none
    return 1 2>/dev/null || exit 1
fi
