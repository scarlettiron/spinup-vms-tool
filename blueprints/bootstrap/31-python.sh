# Scarlett A. Scott
# Created 2026-09-15

# Python tooling.
#
# Two pythons live on this machine, on purpose:
#
#   the system python    whatever the distro ships. dnf, firewalld and half of
#                        /usr/libexec are written against it. It is not touched,
#                        not upgraded, and never made to point somewhere else -
#                        replacing /usr/bin/python3 on a RHEL box is how you end
#                        up with a machine that cannot run dnf to repair itself.
#   the project python   the version this config asks for, installed with uv and
#                        owned by the developer. This is the one you build
#                        against.
#
# uv rather than a distro package, because no distro package answers the
# question. Rocky 9 and RHEL 9 do have python3.14 in AppStream, but Ubuntu 24.04
# tops out at 3.12 - so a package-based approach would silently hand out a
# different Python depending on which blueprint you picked, which is the exact
# thing this repo exists to avoid. uv installs the same CPython build on all of
# them, in about ten seconds, with no compiler.
if ! is_true "${VM_INSTALL_PYTHON:-true}"; then
    log "python tooling disabled"
    return 0 2>/dev/null || exit 0
fi

PYTHON_VERSION="${VM_PYTHON_VERSION:-3.14}"

# The system interpreter plus headers. The headers matter even though the
# project python comes from uv: plenty of wheels still build C extensions
# against whatever python is doing the building.
#
# Named per distro rather than through a translation table: seeing python3-devel
# next to python3-dev is the point, since that pair catches everyone out moving
# between Debian and Fedora. Debian also splits venv into its own package, which
# Fedora includes in python3.
case "$PKG_MANAGER" in
    apt)     pkg_install python3-dev python3-venv python3-pip pipx ;;
    dnf|yum) pkg_install python3 python3-devel python3-pip pipx ;;
    *)       die "no supported package manager for python" ;;
esac

log "configuring pipx + uv for $VM_PRIMARY_USER"
run_as_user "pipx ensurepath >/dev/null 2>&1 || true"
run_as_user "pipx install uv || pipx upgrade uv || true"
run_as_user "pipx install ruff || pipx upgrade ruff || true"

# --------------------------------------------------------------- the project python
if [[ -z "$PYTHON_VERSION" ]]; then
    log "no python version requested, so only the system python is present"
    return 0 2>/dev/null || exit 0
fi

log "installing python ${PYTHON_VERSION} for ${VM_PRIMARY_USER} (uv)"
if ! run_as_user "uv python install '${PYTHON_VERSION}'"; then
    warn "uv could not install python ${PYTHON_VERSION}"
    warn "  check the version exists:  uv python list --all-versions"
    warn "  the system python is still installed and working"
    return 1 2>/dev/null || exit 1
fi

# Put it on PATH as python/python3 FOR THIS USER ONLY.
#
# --default writes python and python3 shims into uv's bin directory, which
# ~/.local/bin already fronts via pipx ensurepath. root, dnf and every system
# service keep the distro interpreter; only the developer's interactive shells
# and their projects see this one. That is the whole point of doing it per-user
# rather than with alternatives(8) or a symlink into /usr/bin.
if run_as_user "uv python install '${PYTHON_VERSION}' --default"; then
    log "python ${PYTHON_VERSION} is the default 'python' for ${VM_PRIMARY_USER}"
else
    # Older uv has no --default. Not fatal: the interpreter is installed and
    # `uv venv --python X` still works, which is how most people use it anyway.
    warn "this uv has no --default, so 'python' still means the system python"
    warn "  use it explicitly:  uv venv --python ${PYTHON_VERSION}"
fi

# Say what actually ended up there, rather than what was asked for. A version
# that silently resolved to something else is worth seeing in the build report.
installed="$(run_as_user "uv python find '${PYTHON_VERSION}' 2>/dev/null" || true)"
if [[ -n "$installed" ]]; then
    log "  interpreter: ${installed}"
    log "  new project:  uv venv --python ${PYTHON_VERSION} && . .venv/bin/activate"
else
    warn "uv reports no python ${PYTHON_VERSION} after installing it"
fi

log "system python left alone: $(python3 --version 2>&1) at $(command -v python3)"
