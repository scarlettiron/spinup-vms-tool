# Scarlett A. Scott
# Created 2026-09-15

# Hyper-V integration services + Enhanced Session Mode.
#
# Enhanced Session gives the VM a resizable desktop, a shared clipboard, and drive
# redirection by tunnelling RDP over an AF_VSOCK hvsocket, so it needs no network
# route to the guest at all. The host half is
# Set-VM -EnhancedSessionTransportType HvSocket (scripts/lib/HyperV.psm1).
#
# Note that $ASSETS resolves to the MERGED assets directory, so each blueprint
# supplies its own xrdp-startwm.sh and its own polkit file. Those genuinely differ
# per distro (locale file, session mode, Xsession path), which is why they stay in
# the blueprint rather than moving here.
ASSETS="${VM_BOOTSTRAP_ROOT:-/usr/local/lib/vm-bootstrap}/assets"

# ------------------------------------------------- integration services (daemons)
# One package on Fedora, two on Ubuntu, and the units are named differently: the
# hv-* names are Debian's, the hyperv* names are Fedora's.
case "$PKG_MANAGER" in
    apt)
        pkg_install linux-tools-virtual linux-cloud-tools-virtual
        hv_services=(hv-kvp-daemon hv-vss-daemon hv-fcopy-daemon)
        ;;
    dnf|yum)
        pkg_install hyperv-daemons
        hv_services=(hypervkvpd hypervvssd hypervfcopyd)
        ;;
    *) die "no supported package manager for the Hyper-V daemons" ;;
esac

for svc in "${hv_services[@]}"; do
    systemctl enable --now "$svc" 2>/dev/null || true
done

if ! is_true "${VM_ENHANCED_SESSION:-true}"; then
    log "enhanced session disabled"
    return 0 2>/dev/null || exit 0
fi
if ! is_true "${VM_INSTALL_DESKTOP:-true}"; then
    log "enhanced session needs a desktop; skipping"
    return 0 2>/dev/null || exit 0
fi

# --------------------------------------------------------------------- xrdp
# Same package names on both.
pkg_install xrdp xorgxrdp

log "pointing xrdp at vsock instead of tcp/3389"

# Edit [Globals] ONLY. This was a plain `sed -i 's|^port=.*|...|'` per setting,
# which is wrong in a way that looks fine until you try to log in: xrdp.ini has
# a `port=` in four sections, and only the one in [Globals] is the listener.
#
#   [Globals]          port=  the address xrdp LISTENS on   <- what we want
#   [Xorg]             port=  -1, meaning "sesman starts it"
#   [vnc-any]          port=  the VNC server's port, 5900
#   [neutrinordp-any]  port=  the target RDP host's port
#
# Rewriting all four pointed the session backends at a vsock address that means
# nothing to them. xrdp still listened, still accepted the connection, still
# asked for credentials - and then failed to start the session, which surfaces
# in the Hyper-V window as
#
#     error - no ip set
#     Error connecting to user session
#
# with nothing in the bootstrap report, because every step genuinely succeeded.
#
# awk rather than sed because the fix needs to know which section it is in, and
# sed has no notion of one.
#
# The second half also REPAIRS a file the old version damaged. A machine built
# before this fix carries the broken xrdp.ini, and re-running the step is the
# natural thing to try; it should heal the machine rather than rewrite the same
# wrong values. A vsock address in a session backend is never a legitimate
# setting - it is the signature of that bug - so those lines are put back, and
# anything else a person may have customised is left alone.
XRDP_INI=/etc/xrdp/xrdp.ini
if [[ ! -f "$XRDP_INI" ]]; then
    # Guard before the rewrite, not after: the edit ends in `cat tmp > ini`,
    # which would happily CREATE an empty xrdp.ini and leave a machine whose
    # xrdp starts with no configuration at all.
    warn "$XRDP_INI is missing, so xrdp cannot be configured for Enhanced Session"
    warn "  did 'pkg_install xrdp xorgxrdp' actually succeed? check further up this log"
    return 1 2>/dev/null || exit 1
fi

xrdp_tmp="$(mktemp)"
awk '
    /^\[/ { section = $0 }

    # The listener.
    section == "[Globals]" {
        if ($0 ~ /^port=/)               { print "port=vsock://-1:3389";      next }
        if ($0 ~ /^use_vsock=/)          { print "use_vsock=false";           next }
        if ($0 ~ /^security_layer=/)     { print "security_layer=rdp";        next }
        if ($0 ~ /^crypt_level=/)        { print "crypt_level=none";          next }
        if ($0 ~ /^bitmap_compression=/) { print "bitmap_compression=false";  next }
        if ($0 ~ /^max_bpp=/)            { print "max_bpp=32";                next }
    }

    # The session backends, healed only when they carry the damage.
    section == "[Xorg]"            && /^port=vsock/ { print "port=-1";   next }
    section == "[Xvnc]"            && /^port=vsock/ { print "port=5900"; next }
    section == "[neutrinordp-any]" && /^port=vsock/ { print "port=ask";  next }

    { print }
' "$XRDP_INI" > "$xrdp_tmp" && cat "$xrdp_tmp" > "$XRDP_INI"
rm -f "$xrdp_tmp"

# Verify, because getting this wrong is invisible until someone opens the VM
# window and gets "Error connecting to user session" with a clean build report.
if ! grep -q '^port=vsock://-1:3389' "$XRDP_INI"; then
    warn "xrdp.ini has no vsock listener after the edit; Enhanced Session will not work"
fi
stray_ports="$(grep -c '^port=vsock' "$XRDP_INI" || true)"
if [[ "$stray_ports" != "1" ]]; then
    warn "expected exactly one vsock port line in xrdp.ini, found ${stray_ports}"
    warn "  a session backend still points at a vsock, so sessions will fail to start"
fi

# ------------------------------------------------- sesman's Xorg log path
#
# Give Xorg an ABSOLUTE log path. The packaged sesman.ini asks for a relative
# one:
#
#     param=-logfile
#     param=.xorgxrdp.%s.log
#
# which only works if Xorg is started with the user's home as its working
# directory. On Rocky 9 it is not - sesman execs it with cwd '/', the user
# cannot write there, and Xorg dies before it has opened a log to say so:
#
#     Fatal server error:
#     (EE) Cannot open log file ".xorgxrdp.78.log"
#
# What the person sees is only "Can't create session for user <name> - X server
# could not be started", with no Xorg log anywhere to explain it, because the
# failure IS the log file. Proven by running the exact command by hand: from the
# home directory it starts and exits cleanly, from / it dies on that line.
#
# %s is the display number, so sessions still get their own file.
SESMAN_INI=/etc/xrdp/sesman.ini
if [[ -f "$SESMAN_INI" ]]; then
    sesman_tmp="$(mktemp)"
    # Only inside [Xorg], and only the line that FOLLOWS '-logfile', so nothing
    # else that happens to look like a path is touched.
    awk '
        /^\[/ { section = $0 }
        section == "[Xorg]" && prev == "param=-logfile" && /^param=/ {
            print "param=/tmp/.xorgxrdp.%s.log"
            prev = $0
            next
        }
        { prev = $0; print }
    ' "$SESMAN_INI" > "$sesman_tmp" && cat "$sesman_tmp" > "$SESMAN_INI"
    rm -f "$sesman_tmp"

    if grep -q '^param=/tmp/\.xorgxrdp\.%s\.log' "$SESMAN_INI"; then
        log "xrdp sessions will log Xorg to /tmp/.xorgxrdp.<display>.log"
    else
        warn "could not set an absolute Xorg log path in $SESMAN_INI"
        warn "  if sessions fail with 'X server could not be started', this is why"
    fi
    selinux_restore "$SESMAN_INI"

    # ------------------------------------------------- prove a session can start
    #
    # Everything above is configuration, and configuration that looks right and
    # is wrong is exactly how this blueprint has failed before: the build report
    # said 22 of 22 steps ok while Enhanced Session could not open a window.
    #
    # So actually start an X server the way sesman will, as the real user, from a
    # working directory the user cannot write - which is the condition that broke
    # it. If this cannot come up, say so here, in the report, rather than leaving
    # it to be discovered in a Hyper-V window with no log to read.
    #
    # KNOW WHAT THIS DOES NOT CATCH. It runs through run_as_user, which is
    # `sudo -iu`, so the X server starts in an unconfined SELinux context. A real
    # session comes through sesman and is confined, and the SELinux gap that
    # broke Rocky 9 (xserver_execmem, handled further down) sails straight past
    # this probe: it reported a clean pass on the very build whose sessions all
    # failed. It proves the command line, the paths and the permissions are
    # right. It does not prove SELinux will allow it.
    #
    # The binary comes out of sesman.ini rather than being hard-coded, because
    # the path differs per distro and sesman.ini already states it correctly.
    xorg_bin="$(awk '/^\[/{s=$0} s=="[Xorg]" && /^param=\// {print $0; exit}' "$SESMAN_INI" | sed 's/^param=//')"
    if [[ -n "$xorg_bin" && -x "$xorg_bin" ]]; then
        log "checking that an X server can actually start (this is what xrdp does)"
        xorg_probe_log=/tmp/.xorgxrdp-selftest.log
        rm -f "$xorg_probe_log"
        # cd / on purpose: it reproduces sesman's working directory. :98 is well
        # clear of X11DisplayOffset=10 and the displays real sessions use.
        if run_as_user "cd / && timeout 15 '$xorg_bin' :98 -config xrdp/xorg.conf -noreset -nolisten tcp -logfile '$xorg_probe_log' >/dev/null 2>&1"; then
            log "  X server started and exited cleanly"
        elif grep -q 'Server terminated successfully' "$xorg_probe_log" 2>/dev/null; then
            # timeout kills it, so a non-zero exit is expected; the log is the truth.
            log "  X server started and was stopped by the probe timeout, which is the pass case"
        else
            warn "  an X server could NOT be started as $VM_PRIMARY_USER"
            warn "  Enhanced Session will fail with 'X server could not be started'."
            if [[ -s "$xorg_probe_log" ]]; then
                warn "  last lines of $xorg_probe_log:"
                grep -E '\(EE\)|Fatal' "$xorg_probe_log" | tail -5 | while IFS= read -r l; do warn "    $l"; done
            else
                warn "  Xorg wrote no log at all, which usually means it could not create one."
            fi
        fi
        rm -f "$xorg_probe_log"
    else
        warn "could not work out the Xorg binary from $SESMAN_INI, so the session check was skipped"
    fi
fi

# Stops GNOME popping polkit auth dialogs the moment an RDP session starts. polkit
# changed format: Debian still reads the old .pkla files from localauthority,
# Fedora wants a JavaScript .rules file.
case "$PKG_MANAGER" in
    apt)
        # xrdp needs to read its TLS key, which lives in the ssl-cert group.
        adduser xrdp ssl-cert >/dev/null 2>&1 || true

        install -d -m 0755 /etc/polkit-1/localauthority/50-local.d
        install -m 0644 "$ASSETS/45-allow-colord.pkla" \
            /etc/polkit-1/localauthority/50-local.d/45-allow-colord.pkla
        ;;
    dnf|yum)
        install -d -m 0755 /etc/polkit-1/rules.d
        install -m 0644 "$ASSETS/45-allow-colord.rules" \
            /etc/polkit-1/rules.d/45-allow-colord.rules
        ;;
esac

# GNOME needs these session hints to come up correctly under xrdp.
install -m 0755 "$ASSETS/xrdp-startwm.sh" /etc/xrdp/startwm.sh

# SELinux is enforcing on Fedora and RHEL. xrdp's own policy ships with the
# package, but the files just dropped in need relabelling or xrdp cannot read
# them. A no-op where SELinux is not enforcing.
selinux_restore /etc/xrdp
if have_cmd setsebool; then
    # xrdp-sesman starts a session bus and writes to the user's home.
    setsebool -P daemons_enable_cluster_mode 1 2>/dev/null || true

    # Xorg wants to execute writable memory. Cheap, and off by default.
    setsebool -P xserver_execmem 1 2>/dev/null \
        || warn "could not set the xserver_execmem SELinux boolean"
fi

# ------------------------------------------- the SELinux rule xrdp actually needs
#
# This is what stopped Enhanced Session working on Rocky 9, and it is invisible
# from anywhere except the audit log. xrdp logs in fine, sesman accepts the
# session, and then the X server dies in the SAME SECOND - not a timeout -
# without writing one line to its own log, because it is killed before it can
# open one. All anybody sees is
#
#     Can't create session for user <name> - X server could not be started
#
# The audit log says what is really happening:
#
#   avc: denied { transition } for comm="xrdp-sesexec" path="/usr/bin/bash"
#   avc: denied { transition } for comm="xrdp-sesexec" path=".../waitforx"
#     scontext=system_u:system_r:unconfined_service_t:s0
#     tcontext=unconfined_u:unconfined_r:unconfined_t:s0  tclass=process
#
# xrdp-sesexec runs as unconfined_service_t and, because /etc/pam.d/xrdp-sesman
# includes `pam_selinux.so open`, tries to transition the session into
# unconfined_t. RHEL's shipped policy has no rule permitting that transition, so
# exec of bash and of waitforx is refused and the session never starts.
#
# There is NO boolean for it - audit2why says plainly "missing type enforcement
# (TE) allow rule" - so it needs a policy module. Both domains involved are
# already unconfined, so this permits a transition between two domains that are
# each unrestricted anyway; it is not a hole in a confinement that existed.
#
# Not 'setenforce 0', which is what every forum thread suggests: that reverts on
# the next reboot, and turning SELinux off on a machine chosen to mirror a Red
# Hat server defeats the point of choosing it.
if [[ "$OS_FAMILY" == "el" ]] && have_cmd semodule && have_cmd checkmodule && have_cmd semodule_package; then
    if semodule -l 2>/dev/null | grep -qx 'vmbootstrap_xrdp'; then
        log "SELinux: the xrdp session-transition module is already installed"
    else
        log "SELinux: building the policy module xrdp needs to start a session"
        se_dir="$(mktemp -d)"
        cat > "$se_dir/vmbootstrap_xrdp.te" <<'POLICY'
module vmbootstrap_xrdp 1.0;

# Let xrdp-sesexec transition a session out of unconfined_service_t into
# unconfined_t, which is what pam_selinux.so asks for in /etc/pam.d/xrdp-sesman.
#
# BOTH rules are needed, and the second is the one that is easy to miss.
#
#   process:transition        the ordinary domain transition. Without it, exec
#                             of bash and of xrdp's waitforx helper is denied.
#   process2:nnp_transition   the same transition, but performed under
#                             no_new_privs. sesman sets NNP on the X server
#                             deliberately - see XorgNoNewPrivileges in
#                             sesman.ini - and under NNP a domain transition
#                             needs this separate permission.
#
# Allowing only the first gets you a session that still fails, with the identical
# "X server could not be started" message and a fresh denial in the audit log.
require {
    type unconfined_t;
    type unconfined_service_t;
    class process transition;
    class process2 nnp_transition;
}

allow unconfined_service_t unconfined_t:process transition;
allow unconfined_service_t unconfined_t:process2 nnp_transition;
POLICY
        if checkmodule -M -m -o "$se_dir/vmbootstrap_xrdp.mod" "$se_dir/vmbootstrap_xrdp.te" >/dev/null 2>&1 \
           && semodule_package -o "$se_dir/vmbootstrap_xrdp.pp" -m "$se_dir/vmbootstrap_xrdp.mod" >/dev/null 2>&1 \
           && semodule -i "$se_dir/vmbootstrap_xrdp.pp" >/dev/null 2>&1; then
            log "  installed: xrdp sessions may start under SELinux enforcing"
        else
            warn "  could not build or install the SELinux module for xrdp"
            warn "  Enhanced Session will fail with 'X server could not be started'."
            warn "  Diagnose with:  sudo ausearch -m avc -ts recent | audit2allow -m x"
        fi
        rm -rf "$se_dir"
    fi
fi

systemctl daemon-reload
systemctl enable xrdp xrdp-sesman
systemctl restart xrdp

log "enhanced session ready - reconnect the VM window after the next reboot"
