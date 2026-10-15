# GitHub access: SSH key, known_hosts, and optionally the gh CLI.
#
# VM_GITHUB_SSH_KEY_MODE:
#   generate - create a keypair here; the private half never existed elsewhere
#   copy     - install the keypair the seed generator staged from the host
#   none     - skip
SECRETS_DIR="${VM_SECRETS_DIR:-/root/vm-bootstrap-secrets}"
MODE="${VM_GITHUB_SSH_KEY_MODE:-none}"
KEY_TYPE="${VM_GITHUB_SSH_KEY_TYPE:-ed25519}"
USER_HOME="$(getent passwd "$VM_PRIMARY_USER" | cut -d: -f6)"
SSH_DIR="$USER_HOME/.ssh"
KEY_PATH="$SSH_DIR/id_${KEY_TYPE}"

install -d -m 0700 -o "$VM_PRIMARY_USER" -g "$VM_PRIMARY_USER" "$SSH_DIR"

# ---------------------------------------------------------------- known_hosts
if is_true "${VM_GITHUB_KNOWN_HOSTS:-true}"; then
    if ! grep -q 'github.com' "$SSH_DIR/known_hosts" 2>/dev/null; then
        log "adding github.com host keys to known_hosts"
        if ssh-keyscan -t rsa,ecdsa,ed25519 github.com >> "$SSH_DIR/known_hosts" 2>/dev/null; then
            chown "$VM_PRIMARY_USER:$VM_PRIMARY_USER" "$SSH_DIR/known_hosts"
            chmod 0644 "$SSH_DIR/known_hosts"
        else
            warn "ssh-keyscan could not reach github.com; the first clone will ask you to confirm the host key"
        fi
    fi
fi

# --------------------------------------------------------------------- the key
case "$MODE" in
    none)
        log "no GitHub SSH key requested"
        ;;
    generate)
        if [[ -f "$KEY_PATH" ]]; then
            log "$KEY_PATH already exists; keeping it"
        else
            comment="${VM_GITHUB_SSH_KEY_COMMENT:-}"
            [[ -n "$comment" ]] || comment="${VM_PRIMARY_USER}@$(hostnamectl --static)"
            log "generating a $KEY_TYPE key for $VM_PRIMARY_USER"
            # No passphrase: this key exists so an unattended local dev VM can
            # talk to GitHub. Use Mode = 'copy' to bring your own passphrased key.
            run_as_user "ssh-keygen -t ${KEY_TYPE} -N '' -C $(printf '%q' "$comment") -f $(printf '%q' "$KEY_PATH")"
        fi
        ;;
    copy)
        staged="$SECRETS_DIR/github-ssh-key"
        if [[ ! -f "$staged" ]]; then
            warn "SshKey.Mode is 'copy' but $staged is missing (shredded by an earlier run?)"
        else
            log "installing the SSH key staged from the host"
            install -m 0600 -o "$VM_PRIMARY_USER" -g "$VM_PRIMARY_USER" "$staged" "$KEY_PATH"
            if [[ -f "$staged.pub" ]]; then
                install -m 0644 -o "$VM_PRIMARY_USER" -g "$VM_PRIMARY_USER" "$staged.pub" "$KEY_PATH.pub"
            else
                # -P '' so a passphrase-protected key fails immediately instead of
                # prompting on a console nobody is watching and hanging bootstrap.
                if ! run_as_user "ssh-keygen -y -P '' -f $(printf '%q' "$KEY_PATH") > $(printf '%q' "$KEY_PATH.pub")"; then
                    rm -f "$KEY_PATH.pub"
                    warn "could not derive a public key from $KEY_PATH (passphrase-protected?)"
                    warn "  the key still works for git, but it will prompt and cannot be uploaded to GitHub"
                fi
            fi
        fi
        ;;
    *)
        warn "unknown VM_GITHUB_SSH_KEY_MODE '$MODE'"
        ;;
esac

# Make sure ssh actually offers this key for github.com.
if [[ -f "$KEY_PATH" ]]; then
    CONF="$SSH_DIR/config"
    if ! grep -q 'Host github.com' "$CONF" 2>/dev/null; then
        log "adding a github.com stanza to ~/.ssh/config"
        {
            echo 'Host github.com'
            echo '    User git'
            echo "    IdentityFile ${KEY_PATH}"
            echo '    IdentitiesOnly yes'
        } >> "$CONF"
        chown "$VM_PRIMARY_USER:$VM_PRIMARY_USER" "$CONF"
        chmod 0600 "$CONF"
    fi
fi

# --------------------------------------------------------------------- gh CLI
if is_true "${VM_GITHUB_INSTALL_CLI:-false}"; then
    # Debian and Ubuntu need GitHub's own apt repo. Fedora ships gh in its
    # standard repos, so there is no third-party repo to add there.
    if [[ "$PKG_MANAGER" == "apt" ]]; then
        add_apt_repo githubcli \
            'https://cli.github.com/packages/githubcli-archive-keyring.gpg' \
            "deb [arch=${GUEST_DEB_ARCH} signed-by=/etc/apt/keyrings/githubcli.gpg] https://cli.github.com/packages stable main"
    fi
    pkg_install gh
fi

TOKEN_FILE="$SECRETS_DIR/github-token"
if [[ -s "$TOKEN_FILE" ]] && have_cmd gh; then
    log "authenticating gh"
    # Piped over stdin, never passed as an argument, so the token stays out of
    # the process list and out of /var/log/vm-bootstrap.log.
    if sudo -iu "$VM_PRIMARY_USER" gh auth login --with-token < "$TOKEN_FILE"; then
        run_as_user "gh auth setup-git" || warn "gh auth setup-git failed"
        run_as_user "gh config set git_protocol ${VM_GITHUB_CLI_PROTOCOL:-ssh}" || true

        if is_true "${VM_GITHUB_UPLOAD_SSH_KEY:-false}" && [[ -f "$KEY_PATH.pub" ]]; then
            title="$(hostnamectl --static) (dev VM)"
            log "registering the public key on the GitHub account"
            run_as_user "gh ssh-key add $(printf '%q' "$KEY_PATH.pub") --title $(printf '%q' "$title")" \
                || warn "could not upload the key - does the token carry the admin:public_key scope?"
        fi
    else
        warn "gh auth login failed; check the token is valid and unexpired"
    fi
elif [[ -s "$TOKEN_FILE" ]]; then
    warn "a GitHub token was supplied but gh is not installed - set GitHub.Cli.Install to true"
fi

# ------------------------------------------------------------------- clean up
if is_true "${VM_GITHUB_SHRED_SECRETS:-true}" && [[ -d "$SECRETS_DIR" ]]; then
    log "shredding staged secrets"
    find "$SECRETS_DIR" -type f -exec shred -u {} + 2>/dev/null || rm -rf "$SECRETS_DIR"
    rmdir "$SECRETS_DIR" 2>/dev/null || true
fi

# Surface the public key so it can be pasted into GitHub when it was not uploaded.
if [[ -f "$KEY_PATH.pub" ]]; then
    log "public key for https://github.com/settings/keys :"
    log "  $(cat "$KEY_PATH.pub")"
fi

# ---------------------------------------------------------------- verification
#
# Whatever route the key arrived by - generated here or copied from the host -
# prove it actually works before declaring the step done, and say which account
# it authenticated as.
#
# GitHub always exits non-zero for `ssh -T` because the account has no shell, so
# the exit code is meaningless here. The greeting text is the only signal:
#
#   Hi <user>! You've successfully authenticated, but GitHub does not provide
#   shell access.
#
# The outcome is written to $AUTH_STATE so 99-finalize.sh can put it in the login
# banner. Reading it off the screen at first login is more use than finding it in
# a log two days later.
AUTH_STATE=/var/lib/vm-bootstrap/github-auth
install -d /var/lib/vm-bootstrap

if [[ ! -f "$KEY_PATH" ]]; then
    log "no SSH key installed, so nothing to verify"
    printf 'skipped\tno ssh key installed\n' > "$AUTH_STATE"
    return 0 2>/dev/null || exit 0
fi

log "verifying the key: ssh -T git@github.com"

# BatchMode stops ssh prompting, so a passphrase-protected key fails here rather
# than hanging a first boot. That failure looks identical to an unregistered key,
# so check for the passphrase first and report the real cause.
key_is_encrypted=0
if ! sudo -iu "$VM_PRIMARY_USER" ssh-keygen -y -P '' -f "$KEY_PATH" >/dev/null 2>&1; then
    key_is_encrypted=1
fi

greeting="$(sudo -iu "$VM_PRIMARY_USER" \
    ssh -T -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new \
    git@github.com 2>&1 || true)"

if [[ "$greeting" == *"successfully authenticated"* ]]; then
    account="$(printf '%s' "$greeting" | sed -n 's/^Hi \([^!]*\)!.*/\1/p' | head -1)"
    log "ssh -T git@github.com: SUCCESS - logged in as '${account:-unknown}'"
    printf 'ok\t%s\n' "${account:-unknown}" > "$AUTH_STATE"

    if [[ -n "${VM_GITHUB_USERNAME:-}" && -n "$account" && "$account" != "$VM_GITHUB_USERNAME" ]]; then
        warn "GitHub greeted '$account' but GitHub.Username says '$VM_GITHUB_USERNAME'"
        warn "  this key belongs to a different account than you configured"
        printf 'mismatch\t%s (configured: %s)\n' "$account" "$VM_GITHUB_USERNAME" > "$AUTH_STATE"
    fi

elif [[ "$greeting" == *"Permission denied"* && $key_is_encrypted -eq 1 ]]; then
    warn "ssh -T git@github.com: FAILED - the key is passphrase-protected"
    warn "  it cannot be used unattended, so this says nothing about whether"
    warn "  GitHub knows the key. Test it yourself after logging in:"
    warn "    ssh -T git@github.com"
    printf 'unknown\tkey is passphrase-protected; verify by hand\n' > "$AUTH_STATE"

elif [[ "$greeting" == *"Permission denied"* ]]; then
    warn "ssh -T git@github.com: FAILED - GitHub does not recognise this key"
    warn "  add the public key at https://github.com/settings/keys, then:"
    warn "    sudo vm-bootstrap --only 55-github --force"
    printf 'denied\tkey not registered on any account\n' > "$AUTH_STATE"

else
    # No network during a first boot is common enough not to shout about.
    log "could not reach github.com to verify the key (offline?)"
    log "  ssh said: $(printf '%s' "$greeting" | head -1)"
    printf 'unreachable\tcould not reach github.com\n' > "$AUTH_STATE"
fi
