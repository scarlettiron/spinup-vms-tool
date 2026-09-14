# Remote access tooling, for reaching client infrastructure from this machine.
#
# Four families, because different clients hand you different things:
#
#   openvpn, wireguard-tools   the open protocols, config files you are given
#   openconnect                Cisco AnyConnect and Palo Alto GlobalProtect
#   freerdp, remmina           reaching Windows servers and other desktops
#   sshuttle                   tunnelling a whole subnet over a plain SSH login
if ! is_true "${VM_INSTALL_REMOTE_ACCESS:-true}"; then
    log "remote access tooling disabled"
    return 0 2>/dev/null || exit 0
fi

# The command line tools carry the same names everywhere, and are useful with
# or without a desktop, so a headless jump box gets them too.
log "installing VPN and tunnelling clients"
pkg_install openvpn wireguard-tools openconnect sshuttle

# Everything below wants a graphical session or NetworkManager, so it is skipped on
# a headless machine rather than pulling a desktop's worth of dependencies onto it.
if is_true "${VM_INSTALL_DESKTOP:-true}"; then
    # The NetworkManager plugins are what let you import a .ovpn or an AnyConnect
    # profile from the GNOME network settings rather than hand-writing a unit.
    # Optional throughout: plugin package names move between releases, and a
    # missing VNC plugin should not cost you the build.
    log "installing NetworkManager VPN plugins and RDP clients"
    case "$PKG_MANAGER" in
        apt)
            pkg_install_optional network-manager-openvpn-gnome network-manager-openconnect-gnome
            # 24.04 carries freerdp3; older releases carry freerdp2. Both are
            # listed and whichever is absent is reported.
            pkg_install_optional freerdp3-x11 freerdp2-x11 remmina remmina-plugin-rdp remmina-plugin-vnc
            ;;
        dnf|yum)
            pkg_install_optional NetworkManager-openvpn-gnome NetworkManager-openconnect-gnome
            pkg_install_optional freerdp remmina remmina-plugins-rdp remmina-plugins-vnc
            ;;
    esac
fi

# Say plainly what is actually usable, since a missing optional package above is
# only a warning and would otherwise be easy to miss.
log "remote access tools available:"
for tool in openvpn wg openconnect sshuttle xfreerdp xfreerdp3 remmina; do
    if have_cmd "$tool"; then
        log "  $tool"
    fi
done

# wg-quick brings up a WireGuard config by name, which is the usual way in, and it
# needs the unit template present rather than just the binary.
if have_cmd wg-quick; then
    log "wireguard: drop a config at /etc/wireguard/<name>.conf, then"
    log "  sudo systemctl enable --now wg-quick@<name>"
fi
