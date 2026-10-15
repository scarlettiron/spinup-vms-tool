# GNOME desktop.
#
# On Fedora the kickstart already selects the Workstation environment, so this
# usually only adds the extras and configures GDM. On Ubuntu the install is a
# server base, so the desktop is installed here in full. Either way the step is
# safe to run when the desktop is already present.
if ! is_true "${VM_INSTALL_DESKTOP:-true}"; then
    log "desktop install disabled"
    return 0 2>/dev/null || exit 0
fi

# ------------------------------------------------------------------ the desktop
case "$PKG_MANAGER" in
    apt)
        DESKTOP_PKG="${VM_DESKTOP_PACKAGE:-ubuntu-desktop}"
        # The desktop meta-package genuinely wants its recommends, unlike
        # everything else here: without them you get a desktop missing half its
        # integration.
        log "installing $DESKTOP_PKG (the slow part - 10+ minutes is normal)"
        pkg_install_with_recommends "$DESKTOP_PKG"
        pkg_install gnome-tweaks xdg-utils fonts-firacode
        ;;
    dnf|yum)
        DESKTOP_PKG="${VM_DESKTOP_PACKAGE:-@workstation-product-environment}"
        if systemctl list-unit-files gdm.service >/dev/null 2>&1 && rpm -q gnome-shell >/dev/null 2>&1; then
            log "GNOME is already installed (kickstart selected it)"
        else
            log "installing $DESKTOP_PKG (the slow part - 10+ minutes is normal)"
            pkg_install_with_recommends "$DESKTOP_PKG"
        fi
        pkg_install gnome-tweaks xdg-utils fira-code-fonts
        ;;
    *) die "no supported package manager for the desktop" ;;
esac

# ------------------------------------------------------------------------- GDM
# Hyper-V's video path has no Wayland story worth relying on, and Enhanced
# Session (xrdp) requires an Xorg session outright. The config lives in
# /etc/gdm3 on Debian and /etc/gdm on Fedora.
GDM_CONF=/etc/gdm/custom.conf
[[ -d /etc/gdm3 ]] && GDM_CONF=/etc/gdm3/custom.conf

log "forcing Xorg in GDM ($GDM_CONF)"
if [[ -f "$GDM_CONF" ]]; then
    if grep -q '^#\?WaylandEnable' "$GDM_CONF"; then
        sed -i 's/^#\?WaylandEnable=.*/WaylandEnable=false/' "$GDM_CONF"
    else
        sed -i '/^\[daemon\]/a WaylandEnable=false' "$GDM_CONF"
    fi
else
    install -d "$(dirname "$GDM_CONF")"
    printf '[daemon]\nWaylandEnable=false\n' > "$GDM_CONF"
fi

# ------------------------------------------------------------ framebuffer mode
# Without an explicit mode the Hyper-V framebuffer comes up at 1024x768 in a basic
# session and cannot be changed from inside the guest.
#
# Two different mechanisms, not just two paths: Fedora uses BLS entries which
# grubby edits per kernel, while Debian regenerates grub.cfg from
# /etc/default/grub. Neither approach works on the other.
#
# Hyper-V only. hyperv_fb is the Hyper-V framebuffer driver; on libvirt and vfkit
# the console is virtio-gpu, which follows the window size by itself, and the
# desktop people use there is xrdp, which takes its size from the RDP client.
RES="${VM_DISPLAY_RESOLUTION:-1920x1080}"
if [[ "${VM_HYPERVISOR:-hyperv}" != "hyperv" ]]; then
    log "not on Hyper-V, so the framebuffer mode is left to the display driver"
    RES=""
fi
if [[ -n "$RES" ]]; then
    log "setting framebuffer resolution to $RES"
    # Chosen by package manager rather than by probing for grubby, so it matches
    # how the rest of this file decides, and so a grubby that happens to be
    # installed on Debian cannot send it down the Fedora path.
    case "$PKG_MANAGER" in
    dnf|yum)
        if have_cmd grubby; then
            grubby --update-kernel=ALL --remove-args="video" 2>/dev/null || true
            grubby --update-kernel=ALL --args="video=hyperv_fb:${RES}" \
                || warn "grubby could not set the video mode"
        else
            warn "grubby is missing, so the framebuffer mode was left alone"
        fi
        ;;
    apt)
        if [[ -f /etc/default/grub ]] && have_cmd update-grub; then
            if grep -q '^GRUB_CMDLINE_LINUX_DEFAULT=' /etc/default/grub; then
                current="$(sed -n 's/^GRUB_CMDLINE_LINUX_DEFAULT="\(.*\)"$/\1/p' /etc/default/grub)"
                cleaned="$(printf '%s' "$current" | sed 's/video=hyperv_fb:[0-9x]*//g' | tr -s ' ')"
                sed -i "s|^GRUB_CMDLINE_LINUX_DEFAULT=.*|GRUB_CMDLINE_LINUX_DEFAULT=\"${cleaned} video=hyperv_fb:${RES}\"|" /etc/default/grub
            else
                ensure_line "GRUB_CMDLINE_LINUX_DEFAULT=\"quiet splash video=hyperv_fb:${RES}\"" /etc/default/grub
            fi
            ensure_line 'GRUB_TIMEOUT=2' /etc/default/grub
            update-grub
        else
            warn "no /etc/default/grub or no update-grub, so the framebuffer mode was left alone"
        fi
        ;;
    esac
fi

systemctl set-default graphical.target
