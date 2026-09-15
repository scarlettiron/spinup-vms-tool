# Scarlett A. Scott
# Created 2026-09-15

# DDEV - container-based local dev environments (Drupal/WordPress/PHP and friends).
if ! is_true "${VM_INSTALL_DDEV:-false}"; then
    log "ddev install disabled"
    return 0 2>/dev/null || exit 0
fi
if ! is_true "${VM_INSTALL_DOCKER:-true}"; then
    warn "ddev needs Docker; enable Bootstrap.InstallDocker"
    return 0 2>/dev/null || exit 0
fi

# From DDEV's own repo rather than the install script, so it upgrades with the
# rest of the system and its version is visible to the package manager.
case "$PKG_MANAGER" in
    apt)
        add_apt_repo ddev \
            'https://pkg.ddev.com/apt/gpg.key' \
            'deb [signed-by=/etc/apt/keyrings/ddev.gpg] https://pkg.ddev.com/apt/ * *'
        pkg_install ddev
        ;;
    dnf|yum)
        # The RPM repo is less well travelled than the apt one, so if it is
        # unreachable or the package is missing, fall back to the installer DDEV
        # documents for Linux rather than failing the whole step.
        add_dnf_repo ddev "[ddev]
name=DDEV
baseurl=https://pkg.ddev.com/yum/
enabled=1
gpgcheck=0"
        if ! pkg_install ddev; then
            warn "the ddev RPM repo did not work; falling back to the documented installer"
            rm -f /etc/yum.repos.d/ddev.repo
            if ! curl -fsSL https://ddev.com/install.sh | bash; then
                warn "ddev install failed; re-run later with: sudo vm-bootstrap --only 33-ddev --force"
                return 0 2>/dev/null || exit 0
            fi
        fi
        ;;
    *) die "no supported package manager for ddev" ;;
esac

# ddev serves projects over HTTPS using a locally trusted CA. mkcert installs that
# CA; the nss tools are what let it register with Firefox and Chrome too, and that
# package is the one name that differs.
case "$PKG_MANAGER" in
    apt)     pkg_install mkcert libnss3-tools ;;
    dnf|yum)
        pkg_install nss-tools
        # Fedora packages mkcert; RHEL and EPEL do not, at any version. Like
        # direnv in 01-base.sh it is a single static Go binary, so take the
        # release rather than lose ddev's HTTPS.
        if [[ "$OS_FAMILY" == "el" ]] && ! have_cmd mkcert; then
            log "installing mkcert from its release binary (no EL package exists)"
            mkcert_ver="$(curl -fsSL https://api.github.com/repos/FiloSottile/mkcert/releases/latest \
                | sed -n 's/.*"tag_name": *"\(v[^"]*\)".*/\1/p' | head -1)"
            if [[ -n "$mkcert_ver" ]] && curl -fsSL \
                    "https://github.com/FiloSottile/mkcert/releases/download/${mkcert_ver}/mkcert-${mkcert_ver}-linux-amd64" \
                    -o /usr/local/bin/mkcert; then
                chmod 0755 /usr/local/bin/mkcert
                selinux_restore /usr/local/bin/mkcert
            else
                warn "could not download mkcert; ddev's HTTPS certificates will be untrusted"
            fi
        elif [[ "$OS_FAMILY" != "el" ]]; then
            pkg_install mkcert
        fi
        ;;
esac

if have_cmd mkcert; then
    log "installing the mkcert local CA for $VM_PRIMARY_USER"
    run_as_user "mkcert -install" || warn "mkcert -install failed; ddev HTTPS will be untrusted until you run it yourself"
else
    warn "mkcert is not installed, so there is no local CA"
    warn "  ddev still works; browsers will warn about its certificates"
fi

# ddev must run as the developer, not root, and needs the docker group that
# 32-docker.sh granted.
run_as_user "ddev version" || warn "ddev is installed but did not report a version"
