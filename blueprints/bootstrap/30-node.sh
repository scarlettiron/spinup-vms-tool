# Scarlett A. Scott
# Created 2026-09-15

# Node via nvm, installed for the primary user (not root).
if ! is_true "${VM_INSTALL_NODE:-true}"; then
    log "node install disabled"
    return 0 2>/dev/null || exit 0
fi

NVM_VERSION="${VM_NVM_VERSION:-v0.40.1}"
NODE_VERSION="${VM_NODE_VERSION:-lts/*}"

log "installing nvm $NVM_VERSION for $VM_PRIMARY_USER"
run_as_user "export PROFILE=\$HOME/.bashrc; curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/${NVM_VERSION}/install.sh | bash"

log "installing node $NODE_VERSION"
run_as_user "export NVM_DIR=\$HOME/.nvm; . \$NVM_DIR/nvm.sh; nvm install '${NODE_VERSION}' && nvm alias default '${NODE_VERSION}' && nvm use default && corepack enable && node --version && npm --version"
