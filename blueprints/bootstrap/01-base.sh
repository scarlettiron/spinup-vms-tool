# Scarlett A. Scott
# Created 2026-09-15

# Base packages, locale, timezone, and a full upgrade.

# Before anything else on a RHEL rebuild: half the list below is in EPEL, not in
# the OS. A no-op everywhere else.
enable_epel

pkg_upgrade_all

# The two lists are the same toolkit under different names. Kept side by side and
# spelled out rather than mapped, because the interesting cases are exactly the
# ones a table would hide: build-essential is a metapackage while Fedora wants a
# group, openssh-client is singular on Debian and plural on Fedora, and Debian
# splits python venv into its own package.
case "$PKG_MANAGER" in
    apt)
        pkg_install \
            build-essential pkg-config \
            git git-lfs curl wget ca-certificates gnupg lsb-release \
            openssh-client openssh-server \
            jq unzip zip tar rsync \
            htop tree less nano vim \
            ripgrep fd-find fzf tmux direnv \
            software-properties-common apt-transport-https \
            net-tools dnsutils iputils-ping traceroute \
            python3 python3-venv python3-pip pipx

        # Ubuntu ships fd as fdfind to avoid a name clash; give people the usual name.
        if have_cmd fdfind && [[ ! -e /usr/local/bin/fd ]]; then
            ln -s "$(command -v fdfind)" /usr/local/bin/fd
        fi
        ;;
    dnf|yum)
        # Two dnf distros, and three names genuinely differ. The comps group is
        # 'development-tools' on Fedora and plain 'development' on RHEL, and
        # Fedora's 'vim' metapackage does not exist on RHEL, where the usable
        # vim is 'vim-enhanced'.
        case "$OS_FAMILY" in
            el) dev_group='@development'; vim_pkg='vim-enhanced' ;;
            *)  dev_group='@development-tools'; vim_pkg='vim' ;;
        esac

        pkg_install \
            "$dev_group" pkgconf-pkg-config \
            git git-lfs curl wget ca-certificates gnupg2 \
            openssh-clients openssh-server \
            jq unzip zip tar rsync \
            htop tree less nano "$vim_pkg" \
            ripgrep fd-find fzf tmux \
            bind-utils iputils traceroute net-tools \
            policycoreutils-python-utils

        # direnv is packaged on Fedora and, as of EL9, nowhere in RHEL, EPEL or
        # CRB. It is a single static Go binary, so fetch the release rather than
        # leave a hole in the shell setup that 60-dotfiles.sh expects to fill.
        if [[ "$OS_FAMILY" == "el" ]]; then
            if have_cmd direnv; then
                log "direnv already installed"
            else
                log "installing direnv from its release binary (no EL9 package exists)"
                if curl -fsSL https://github.com/direnv/direnv/releases/latest/download/direnv.linux-amd64 \
                        -o /usr/local/bin/direnv; then
                    chmod 0755 /usr/local/bin/direnv
                    selinux_restore /usr/local/bin/direnv
                    have_cmd direnv || warn "direnv downloaded but does not run"
                else
                    warn "could not download direnv; shells will start without it"
                fi
            fi
        else
            pkg_install direnv
        fi
        ;;
    *) die "no supported package manager" ;;
esac

if [[ -n "${VM_TIMEZONE:-}" ]]; then
    log "setting timezone to $VM_TIMEZONE"
    timedatectl set-timezone "$VM_TIMEZONE" || warn "could not set timezone"
fi

if [[ -n "${VM_LOCALE:-}" ]]; then
    log "setting locale to $VM_LOCALE"
    case "$PKG_MANAGER" in
        apt)
            locale-gen "$VM_LOCALE" >/dev/null 2>&1 || true
            update-locale "LANG=$VM_LOCALE" || warn "could not set locale"
            ;;
        dnf|yum)
            # Fedora's minimal installs ship glibc-langpack-en only; localectl needs
            # the matching langpack present or the setting silently does nothing.
            pkg_install glibc-langpack-en || true
            localectl set-locale "LANG=$VM_LOCALE" || warn "could not set locale"
            ;;
    esac
fi

if [[ -n "${VM_EXTRA_APT_PACKAGES:-}" ]]; then
    # Named for apt but it is just "extra packages"; the name is kept so one config
    # key works for every blueprint.
    # shellcheck disable=SC2086
    pkg_install $VM_EXTRA_APT_PACKAGES
fi

if is_true "${VM_SSH_INSTALL_SERVER:-true}"; then
    # SELinux is enforcing on Fedora and the drop-in directory it ships is 0700
    # with 0600 files, so match the stricter of the two everywhere rather than
    # loosening one distro to look like the other.
    install -d -m 0700 /etc/ssh/sshd_config.d
    if is_true "${VM_SSH_ALLOW_PASSWORD_AUTH:-true}"; then
        printf 'PasswordAuthentication yes\n' > /etc/ssh/sshd_config.d/60-vm-bootstrap.conf
    else
        printf 'PasswordAuthentication no\nPubkeyAuthentication yes\n' > /etc/ssh/sshd_config.d/60-vm-bootstrap.conf
    fi
    chmod 0600 /etc/ssh/sshd_config.d/60-vm-bootstrap.conf

    # The unit is 'ssh' on Debian and 'sshd' on Fedora. Each ships an alias for the
    # other, but not dependably across releases, so name it outright rather than
    # probing: the same branch that chose the package names knows the answer.
    case "$PKG_MANAGER" in
        apt)     ssh_unit=ssh ;;
        dnf|yum) ssh_unit=sshd ;;
    esac
    log "enabling $ssh_unit"
    systemctl enable "$ssh_unit"
    systemctl restart "$ssh_unit"

    # firewalld is active on Fedora; a no-op where it is not running.
    firewall_allow_service ssh
fi
