# Docker Engine from Docker's own repository, rather than the distro's docker.io
# or moby-engine, so the team gets current Engine + Compose v2 + Buildx and the
# same versions on every OS.
if ! is_true "${VM_INSTALL_DOCKER:-true}"; then
    log "docker install disabled"
    return 0 2>/dev/null || exit 0
fi

# Same package names on both, so only the repo definition differs.
case "$PKG_MANAGER" in
    apt)
        # Docker publishes per-release repos, so the codename has to be looked up
        # rather than assumed.
        codename="$(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}")"
        add_apt_repo docker \
            'https://download.docker.com/linux/ubuntu/gpg' \
            "deb [arch=${GUEST_DEB_ARCH} signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu ${codename} stable"
        ;;
    dnf|yum)
        # Docker builds per distro, not per package manager: Fedora and RHEL get
        # separate trees, and there is no Rocky tree at all - the RHEL rebuilds
        # use the CentOS one, which is what they are binary-compatible with.
        #
        # The RHEL tree is keyed on the major version, so $OS_MAJOR is used
        # rather than dnf's $releasever: on a point release the two agree, but
        # only one of them is guaranteed not to be '9.8'.
        case "$OS_FAMILY" in
            el) docker_tree="centos/${OS_MAJOR}"; docker_gpg='centos' ;;
            *)  docker_tree='fedora/$releasever'; docker_gpg='fedora' ;;
        esac

        # $basearch is dnf's own variable, escaped so the shell leaves it for dnf
        # to expand. $releasever, where it appears above, is single-quoted for
        # the same reason.
        add_dnf_repo docker-ce "[docker-ce-stable]
name=Docker CE Stable
baseurl=https://download.docker.com/linux/${docker_tree}/\$basearch/stable
enabled=1
gpgcheck=1
gpgkey=https://download.docker.com/linux/${docker_gpg}/gpg"
        ;;
    *) die "no supported package manager for docker" ;;
esac

pkg_install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin

systemctl enable --now docker

log "adding $VM_PRIMARY_USER to the docker group"
usermod -aG docker "$VM_PRIMARY_USER"

docker --version
docker compose version
