# Sourced by every bootstrap step. Not a step itself.
# shellcheck shell=bash

export DEBIAN_FRONTEND=noninteractive
export NEEDRESTART_MODE=a

log()  { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
warn() { printf '[%s] WARN: %s\n' "$(date +%H:%M:%S)" "$*" >&2; }
die()  { printf '[%s] ERROR: %s\n' "$(date +%H:%M:%S)" "$*" >&2; exit 1; }

have_cmd() { command -v "$1" >/dev/null 2>&1; }

# Treat 1/true/yes/on as true so the value can come from PowerShell or a human.
is_true() {
    case "${1:-}" in
        1|true|True|TRUE|yes|Yes|on|On) return 0 ;;
        *) return 1 ;;
    esac
}

# ---------------------------------------------------------------- packages
#
# Every step lives here and runs on every OS. Where the packages are named the
# same, these wrappers are all a step needs. Where the names differ, the step
# branches on $PKG_MANAGER and writes both lists out in full:
#
#     case "$PKG_MANAGER" in
#         apt)     pkg_install python3-dev python3-venv python3-pip ;;
#         dnf|yum) pkg_install python3 python3-devel python3-pip ;;
#     esac
#
# Two lists side by side, not a translation table. Seeing python3-devel next to
# python3-dev is the point: a table that quietly maps one to the other hides
# exactly the cases you need to look at, and says nothing useful when a package
# exists on one distro and not the other.
#
# Where two dnf distros differ from each other, branch again on $OS_FAMILY
# inside the dnf arm - see the block below detect_pkg_manager.
#
# A blueprint can still override any step by dropping a file of the same name in
# its own bootstrap/ directory, since later directories win when the trees are
# layered. That is for an OS that needs different LOGIC, not different names.
detect_pkg_manager() {
    if have_cmd apt-get; then echo apt
    elif have_cmd dnf; then echo dnf
    elif have_cmd yum; then echo yum
    else echo unknown
    fi
}
PKG_MANAGER="$(detect_pkg_manager)"
export PKG_MANAGER

# $PKG_MANAGER alone is not enough once an enterprise rebuild is in the mix.
# Fedora and Rocky both use dnf, but they are not the same package set: Rocky is
# RHEL, which ships a deliberately small repo and pushes everything else into
# EPEL. On Rocky, ripgrep, fzf, pipx, openvpn, remmina and - the one that really
# matters - xrdp are all EPEL packages. Without it there is no Enhanced Session.
#
# So there is a second axis. $OS_FAMILY is the one to branch on:
#
#   el       RHEL and its rebuilds: Rocky, Alma, CentOS Stream
#   fedora   Fedora
#   debian   Debian and Ubuntu
#
# Use $PKG_MANAGER for apt-vs-dnf and $OS_FAMILY only for the handful of places
# where two dnf distros genuinely differ. Branching on $OS_ID (the exact distro)
# is a last resort - it means the step stops working the day someone adds Alma.
detect_os_family() {
    case "${OS_ID}" in
        fedora) echo fedora ;;
        rhel|rocky|almalinux|centos|ol) echo el ;;
        debian|ubuntu) echo debian ;;
        *)
            # ID_LIKE is how a derivative declares its parent, so an unknown
            # rebuild still lands in the right family.
            case " ${OS_ID_LIKE} " in
                *" rhel "*|*" centos "*) echo el ;;
                *" fedora "*)            echo fedora ;;
                *" debian "*)            echo debian ;;
                *)                       echo unknown ;;
            esac
            ;;
    esac
}
OS_ID="$(. /etc/os-release 2>/dev/null && echo "${ID:-unknown}")"
OS_ID_LIKE="$(. /etc/os-release 2>/dev/null && echo "${ID_LIKE:-}")"
OS_VERSION_ID="$(. /etc/os-release 2>/dev/null && echo "${VERSION_ID:-}")"
# The major version on its own. RHEL-family repo URLs and the dnf $releasever
# both want '9', not '9.8'.
OS_MAJOR="${OS_VERSION_ID%%.*}"
OS_FAMILY="$(detect_os_family)"
export OS_ID OS_ID_LIKE OS_VERSION_ID OS_MAJOR OS_FAMILY

# The guest's CPU architecture, in the two spellings downloads use:
#
#   GUEST_ARCH      x86_64 | aarch64   the kernel's names, used by rpm repos
#   GUEST_DEB_ARCH  amd64  | arm64     Debian's names, used by apt lines and by
#                                      most GitHub release assets
#
# The host says which in VM_ARCH; a seed from the PowerShell tool does not, so
# fall back to asking the kernel, which is never wrong about itself.
GUEST_ARCH="${VM_ARCH:-$(uname -m)}"
case "$GUEST_ARCH" in
    aarch64|arm64) GUEST_ARCH=aarch64; GUEST_DEB_ARCH=arm64 ;;
    *)             GUEST_ARCH=x86_64;  GUEST_DEB_ARCH=amd64 ;;
esac
export GUEST_ARCH GUEST_DEB_ARCH

pkg_update_once() {
    [[ -f /var/lib/vm-bootstrap/pkg-updated ]] && return 0
    mkdir -p /var/lib/vm-bootstrap
    case "$PKG_MANAGER" in
        apt)
            log "apt-get update"
            apt-get update -y
            ;;
        dnf|yum)
            # dnf refreshes its own metadata per transaction, so there is no
            # separate update step to run - just prime the cache once.
            log "dnf makecache"
            dnf -y makecache || true
            ;;
        *) die "no supported package manager found" ;;
    esac
    touch /var/lib/vm-bootstrap/pkg-updated
}

pkg_install() {
    [[ $# -gt 0 ]] || return 0
    pkg_update_once
    log "installing: $*"
    case "$PKG_MANAGER" in
        apt)
            apt-get install -y --no-install-recommends \
                -o Dpkg::Options::=--force-confdef \
                -o Dpkg::Options::=--force-confold \
                "$@"
            ;;
        dnf|yum)
            dnf install -y --setopt=install_weak_deps=False "$@"
            ;;
        *) die "no supported package manager found" ;;
    esac
}

# Install packages that may not exist under the name given on this release, one at
# a time so a single wrong name does not take the rest down with it. Reports what
# it could not find instead of failing the step, which is the right trade for
# nice-to-have tooling: a missing VPN plugin should not cost you the whole build.
#
# Use pkg_install for anything the machine genuinely needs.
pkg_install_optional() {
    [[ $# -gt 0 ]] || return 0
    pkg_update_once
    local pkg missing=()
    for pkg in "$@"; do
        case "$PKG_MANAGER" in
            apt)
                apt-get install -y --no-install-recommends \
                    -o Dpkg::Options::=--force-confdef \
                    -o Dpkg::Options::=--force-confold "$pkg" >/dev/null 2>&1 \
                    || missing+=("$pkg")
                ;;
            dnf|yum)
                dnf install -y --setopt=install_weak_deps=False "$pkg" >/dev/null 2>&1 \
                    || missing+=("$pkg")
                ;;
            *) return 0 ;;
        esac
    done
    local installed=$(( $# - ${#missing[@]} ))
    log "installed $installed of $# optional package(s)"
    if [[ ${#missing[@]} -gt 0 ]]; then
        warn "not available on this release: ${missing[*]}"
    fi
}

# Install with recommended/weak dependencies, which a desktop group needs.
pkg_install_with_recommends() {
    [[ $# -gt 0 ]] || return 0
    pkg_update_once
    log "installing (with recommends): $*"
    case "$PKG_MANAGER" in
        apt)
            apt-get install -y \
                -o Dpkg::Options::=--force-confdef \
                -o Dpkg::Options::=--force-confold \
                "$@"
            ;;
        dnf|yum) dnf install -y "$@" ;;
        *) die "no supported package manager found" ;;
    esac
}

pkg_upgrade_all() {
    pkg_update_once
    log "applying pending upgrades"
    case "$PKG_MANAGER" in
        apt)
            apt-get -y -o Dpkg::Options::=--force-confdef \
                       -o Dpkg::Options::=--force-confold dist-upgrade
            ;;
        dnf|yum) dnf -y upgrade --refresh ;;
        *) die "no supported package manager found" ;;
    esac
}

pkg_cleanup() {
    case "$PKG_MANAGER" in
        apt) apt-get autoremove -y; apt-get clean ;;
        dnf|yum) dnf -y autoremove || true; dnf clean all || true ;;
    esac
}

# Run a command as the primary desktop user, with a login shell so ~/.profile
# and ~/.bashrc (nvm, cargo, etc.) are in scope, starting in their home.
#
# Not `sudo -i`: that hands the command to the user's shell, which expands
# every $VAR in it before bash -lc runs, so "export NVM_DIR=...; . \$NVM_DIR/x"
# arrived as ". /x" (sudo 1.9 on Ubuntu 24.04). One shell, one expansion.
run_as_user() {
    local user="${VM_PRIMARY_USER:?VM_PRIMARY_USER not set}"
    sudo -u "$user" -H bash -lc "cd \"\$HOME\" && $*"
}

# Append a line to a file only if it is not already present.
ensure_line() {
    local line="$1" file="$2"
    touch "$file"
    grep -qxF "$line" "$file" || printf '%s\n' "$line" >> "$file"
}

# Install an apt keyring + repo idempotently. apt only; dnf steps write a .repo
# file directly, which is simpler than mirroring this.
add_apt_repo() {
    local name="$1" key_url="$2" repo_line="$3"
    local keyring="/etc/apt/keyrings/${name}.gpg" tmp
    install -d -m 0755 /etc/apt/keyrings

    if [[ ! -s "$keyring" ]]; then
        log "adding apt key for ${name}"
        tmp="$(mktemp)"
        curl -fsSL "$key_url" -o "$tmp"
        # Vendors are inconsistent: Docker and DDEV publish an ASCII-armored
        # key that needs dearmoring, while GitHub CLI publishes a binary
        # keyring that gpg --dearmor would reject.
        if head -c 64 "$tmp" | grep -q 'BEGIN PGP'; then
            gpg --dearmor < "$tmp" > "$keyring"
        else
            cp "$tmp" "$keyring"
        fi
        rm -f "$tmp"
        chmod 0644 "$keyring"
    fi

    printf '%s\n' "$repo_line" > "/etc/apt/sources.list.d/${name}.list"
    # A new repo means the package lists are stale again.
    rm -f /var/lib/vm-bootstrap/pkg-updated
}

# Write a dnf repo definition idempotently.
add_dnf_repo() {
    local name="$1" file_contents="$2"
    printf '%s\n' "$file_contents" > "/etc/yum.repos.d/${name}.repo"
    rm -f /var/lib/vm-bootstrap/pkg-updated
}

# Enable EPEL, on the distros that need it.
#
# RHEL ships a small repo on purpose, so a rebuild of it does too. On Rocky 9
# these all live in EPEL rather than the OS: xrdp and xorgxrdp, nss-mdns,
# ripgrep, fd-find, fzf, pipx, htop, openvpn, openconnect, sshuttle, remmina,
# the NetworkManager VPN plugins, gh, and the Fira Code fonts. Losing xrdp
# means losing Enhanced Session Mode, so this is not optional decoration.
#
# A no-op on Fedora, which has all of that in its own repos, and on Debian.
#
# CRB (CodeReady Builder, called PowerTools before RHEL 9) carries build-time
# libraries that some EPEL packages link against, and EPEL's own instructions
# say to turn it on first. It is present but disabled by default.
enable_epel() {
    [[ "$OS_FAMILY" == "el" ]] || return 0
    if rpm -q epel-release >/dev/null 2>&1; then
        log "EPEL is already enabled"
        return 0
    fi

    log "enabling CRB and EPEL (RHEL ships a small repo; the dev tooling is in EPEL)"
    # dnf config-manager lives here, and a minimal install may not have it.
    have_cmd dnf || die "enable_epel needs dnf"
    rpm -q dnf-plugins-core >/dev/null 2>&1 || dnf install -y dnf-plugins-core

    # CRB is reached differently on Red Hat's own build than on the rebuilds.
    #
    # On Rocky, Alma and CentOS Stream it is an ordinary repo that ships
    # disabled, so dnf config-manager turns it on. On subscription-managed RHEL
    # it is an entitled repository, and config-manager cannot enable it at all:
    # it has to come from subscription-manager, and only once the machine is
    # registered. This is one of the few places the exact distro matters rather
    # than the family.
    if [[ "$OS_ID" == "rhel" ]]; then
        if subscription-manager identity >/dev/null 2>&1; then
            log "enabling CodeReady Builder through subscription-manager"
            subscription-manager repos --enable "codeready-builder-for-rhel-${OS_MAJOR}-$(arch)-rpms" >/dev/null 2>&1 \
                || warn "could not enable CodeReady Builder; some EPEL packages will fail on their build dependencies"
        else
            warn "this RHEL machine is not registered, so CodeReady Builder cannot be enabled"
            warn "  EPEL packages that depend on it will fail to install"
        fi
    else
        # 'crb' on 9 and later, 'powertools' on 8. Try both and do not fail on
        # the one that does not exist here.
        dnf config-manager --set-enabled crb >/dev/null 2>&1 \
            || dnf config-manager --set-enabled powertools >/dev/null 2>&1 \
            || warn "could not enable CRB; a few EPEL packages may fail to install"
    fi

    # epel-release is in Rocky's own 'extras' repo, which is enabled by default,
    # so there it needs no URL. Red Hat does not ship it at all, so that path
    # goes straight to the Fedora-hosted rpm, which is what EPEL's own
    # instructions for RHEL say to use.
    if [[ "$OS_ID" != "rhel" ]] && dnf install -y epel-release; then
        :
    else
        [[ "$OS_ID" == "rhel" ]] || warn "epel-release is not in this distribution's repos; installing it from Fedora"
        dnf install -y "https://dl.fedoraproject.org/pub/epel/epel-release-latest-${OS_MAJOR}.noarch.rpm" \
            || die "could not enable EPEL, and this blueprint needs it for xrdp and most of the dev tooling"
    fi

    # New repo, so the cached metadata no longer describes what is installable.
    rm -f /var/lib/vm-bootstrap/pkg-updated
}

# Open a firewalld service or port when firewalld is running. A no-op on Ubuntu,
# which ships firewalld absent and ufw inactive.
firewall_allow_service() {
    local service="$1"
    have_cmd firewall-cmd || return 0
    systemctl is-active --quiet firewalld || return 0
    log "opening firewalld service '$service'"
    firewall-cmd --permanent --add-service="$service" >/dev/null 2>&1 || true
    firewall-cmd --reload >/dev/null 2>&1 || true
}

# Open a single TCP port in firewalld. A no-op on Ubuntu, which ships firewalld
# absent and ufw inactive, so nothing there needs opening.
firewall_allow_port() {
    local port="$1"
    have_cmd firewall-cmd || return 0
    systemctl is-active --quiet firewalld || return 0
    log "opening firewalld port ${port}/tcp"
    firewall-cmd --permanent --add-port="${port}/tcp" >/dev/null 2>&1 || true
    firewall-cmd --reload >/dev/null 2>&1 || true
}

# Let a confined service bind a port it does not own by default.
#
# SELinux labels ports, not just files: postgresql_t may bind postgresql_port_t
# and nothing else, so moving Postgres to port 5433 is denied even with the
# firewall open, and the error surfaces as the daemon failing to start rather than
# as anything mentioning SELinux. Nothing to do when the port already carries the
# type, which is the case for every default port.
selinux_allow_port() {
    local type="$1" port="$2"
    [[ -n "$type" && -n "$port" ]] || return 0
    have_cmd getenforce || return 0
    [[ "$(getenforce 2>/dev/null)" == "Enforcing" ]] || return 0

    if ! have_cmd semanage; then
        # Fedora splits semanage out of the base install.
        pkg_install policycoreutils-python-utils || {
            warn "could not install policycoreutils-python-utils, so port $port cannot be labelled for SELinux"
            return 0
        }
    fi

    if semanage port -l 2>/dev/null | grep -qE "^${type}[[:space:]]+tcp.*\b${port}\b"; then
        log "SELinux already allows $type on port $port"
        return 0
    fi

    log "labelling port $port as $type for SELinux"
    semanage port -a -t "$type" -p tcp "$port" 2>/dev/null \
        || semanage port -m -t "$type" -p tcp "$port" 2>/dev/null \
        || warn "could not label port $port as $type; the service may be denied the bind"
}

# Relabel a path for SELinux when SELinux is enforcing. A no-op on Ubuntu.
selinux_restore() {
    local path="$1"
    have_cmd restorecon || return 0
    restorecon -R "$path" 2>/dev/null || true
}

# The group that grants sudo: 'wheel' on Fedora/RHEL, 'sudo' on Debian/Ubuntu.
admin_group() {
    if getent group wheel >/dev/null 2>&1; then echo wheel
    else echo sudo
    fi
}
