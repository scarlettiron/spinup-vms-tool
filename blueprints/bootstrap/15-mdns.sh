# mDNS / zeroconf so the VMs on this virtual switch can reach each other by name
# without static IPs or hand-maintained hosts files.
#
# Avahi publishes the system hostname in the .local domain, and the nss-mdns
# plugin wires .local into glibc name resolution, so `ping other-vm.local` works
# from any other VM on the same Hyper-V switch. Windows 10+ resolves .local via
# its own mDNS responder, so the host can reach the guests too.
if ! is_true "${VM_ENABLE_MDNS:-true}"; then
    log "mDNS disabled"
    return 0 2>/dev/null || exit 0
fi

# The installer already set this from Guest.Hostname; enforce it in case the name
# was changed in config and bootstrap re-run.
if [[ -n "${VM_HOSTNAME:-}" && "$(hostnamectl --static)" != "$VM_HOSTNAME" ]]; then
    log "setting hostname to $VM_HOSTNAME"
    hostnamectl set-hostname "$VM_HOSTNAME"
    ensure_line "127.0.1.1 ${VM_HOSTNAME}" /etc/hosts
fi

# The same three components under different names on each distro.
case "$PKG_MANAGER" in
    apt)     pkg_install avahi-daemon avahi-utils libnss-mdns ;;
    dnf|yum) pkg_install avahi avahi-tools nss-mdns ;;
    *)       die "no supported package manager for avahi" ;;
esac

# Publish the workstation itself; leave the service-discovery extras off so we are
# not advertising a printer or an ssh service to the whole switch unasked.
if [[ -f /etc/avahi/avahi-daemon.conf ]]; then
    sed -i 's/^#\?publish-workstation=.*/publish-workstation=yes/' /etc/avahi/avahi-daemon.conf
    grep -q '^publish-workstation=' /etc/avahi/avahi-daemon.conf \
        || sed -i '/^\[publish\]/a publish-workstation=yes' /etc/avahi/avahi-daemon.conf
fi

# Debian's libnss-mdns postinst usually adds this already, and Fedora's does not.
# Verify rather than assume, on both.
if ! grep -q 'mdns' /etc/nsswitch.conf; then
    log "adding mdns to the nsswitch hosts line"
    sed -i 's/^hosts:\(.*\)$/hosts:\1 mdns4_minimal [NOTFOUND=return]/' /etc/nsswitch.conf
fi

systemctl enable --now avahi-daemon

# mDNS is multicast UDP 5353. Fedora runs firewalld, which has a service
# definition for it; Ubuntu ships ufw inactive, so there is only something to open
# if someone has turned it on.
firewall_allow_service mdns
if have_cmd ufw && ufw status 2>/dev/null | grep -q '^Status: active'; then
    log "opening 5353/udp for mDNS"
    ufw allow 5353/udp comment 'mDNS' || true
fi

log "this machine is reachable as $(hostnamectl --static).local"

# Any peers named in config get a quick reachability report in the log - purely
# informational, a peer that is not built yet is expected to fail.
if [[ -n "${VM_MDNS_PEERS:-}" ]]; then
    for peer in $VM_MDNS_PEERS; do
        [[ "$peer" == "$(hostnamectl --static)" ]] && continue
        if getent hosts "${peer}.local" >/dev/null 2>&1; then
            log "peer ${peer}.local resolves to $(getent hosts "${peer}.local" | awk '{print $1}' | head -1)"
        else
            log "peer ${peer}.local not resolvable yet (not built, or still installing)"
        fi
    done
fi
