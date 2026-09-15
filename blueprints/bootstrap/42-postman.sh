# Scarlett A. Scott
# Created 2026-09-15

# Postman, from the vendor's own tarball.
#
# Off by default: it is a 200 MB Electron app and plenty of people use curl, the
# VS Code REST client, or Insomnia instead. Turn it on with installPostman.
#
# The tarball rather than a package, because there is no package that works
# everywhere: Postman ships a snap (Ubuntu only) and a flatpak (needs flathub
# wired up), and neither exists for Rocky or RHEL out of the box. The tarball is
# the one artefact the vendor publishes for all of them, so every blueprint gets
# the identical build - which is the same reasoning as uv for python.
if ! is_true "${VM_INSTALL_POSTMAN:-false}"; then
    log "postman not requested"
    return 0 2>/dev/null || exit 0
fi

# An API client with no desktop to draw it in is 200 MB of nothing. The server
# blueprint turns the desktop off, so follow that rather than needing a second
# switch set to match - same rule as chrome.
if ! is_true "${VM_INSTALL_DESKTOP:-true}"; then
    log "no desktop on this machine, so skipping postman"
    return 0 2>/dev/null || exit 0
fi

POSTMAN_URL="${VM_POSTMAN_URL:-https://dl.pstmn.io/download/latest/linux_64}"
POSTMAN_DIR=/opt/Postman

if [[ -x "$POSTMAN_DIR/Postman" ]]; then
    log "postman already installed at $POSTMAN_DIR"
    return 0 2>/dev/null || exit 0
fi

# Electron needs these at runtime. They usually arrive with GNOME, but the
# server-plus-desktop blueprints can be thinner, and a missing libatk turns into
# a window that never appears rather than an error anyone can read.
case "$PKG_MANAGER" in
    apt)     pkg_install_optional libgtk-3-0 libnss3 libatk-bridge2.0-0 libgbm1 libasound2t64 libasound2 ;;
    dnf|yum) pkg_install_optional gtk3 nss atk at-spi2-atk mesa-libgbm alsa-lib ;;
esac

tarball="$(mktemp /tmp/postman.XXXXXX.tar.gz)"
log "downloading postman (about 200 MB)"
if ! curl -fsSL "$POSTMAN_URL" -o "$tarball"; then
    warn "could not download postman from $POSTMAN_URL"
    rm -f "$tarball"
    return 1 2>/dev/null || exit 1
fi

# Check it really is a gzip before unpacking. A captive portal or an error page
# would otherwise land as a 2 KB "tarball" and fail with something obscure.
if [[ "$(head -c 2 "$tarball" | od -An -tx1 | tr -d ' ')" != "1f8b" ]]; then
    warn "what downloaded is not a gzip archive - a redirect or an error page?"
    warn "  size: $(stat -c %s "$tarball") bytes"
    rm -f "$tarball"
    return 1 2>/dev/null || exit 1
fi

log "unpacking to $POSTMAN_DIR"
rm -rf "$POSTMAN_DIR"
install -d -m 0755 /opt
# The archive has a single Postman/ at its root, so strip nothing and let it
# create /opt/Postman itself.
if ! tar -xzf "$tarball" -C /opt; then
    warn "could not unpack the postman tarball"
    rm -f "$tarball"
    return 1 2>/dev/null || exit 1
fi
rm -f "$tarball"

if [[ ! -x "$POSTMAN_DIR/Postman" ]]; then
    warn "unpacked, but $POSTMAN_DIR/Postman is not there - has the archive layout changed?"
    return 1 2>/dev/null || exit 1
fi

# On PATH, and in the applications menu.
ln -sf "$POSTMAN_DIR/Postman" /usr/local/bin/postman

install -d -m 0755 /usr/share/applications
cat > /usr/share/applications/postman.desktop <<DESKTOP
[Desktop Entry]
Type=Application
Name=Postman
Comment=API development and testing
Exec=$POSTMAN_DIR/Postman %U
Icon=$POSTMAN_DIR/app/resources/app/assets/icon.png
Terminal=false
Categories=Development;Network;
StartupWMClass=Postman
DESKTOP
chmod 0644 /usr/share/applications/postman.desktop

# SELinux labels a file by where it was created, and this one was written by
# bootstrap rather than by a package. A no-op where SELinux is not enforcing.
selinux_restore /opt/Postman
selinux_restore /usr/share/applications/postman.desktop

log "postman installed: $(command -v postman)"
log "  launch it from the applications menu, or run: postman"
