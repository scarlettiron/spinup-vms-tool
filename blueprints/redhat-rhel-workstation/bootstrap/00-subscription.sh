# Register this machine with Red Hat, so it has package repositories at all.
#
# RHEL-only, which is why this step lives in the blueprint rather than in the
# shared tree: Rocky, Alma and CentOS Stream have public repos and need none of
# it. Numbered 00 so it runs before 01-base.sh, the first step that wants to
# install anything.
#
#   VM_RHSM_MODE         activation-key | credentials | none
#   VM_RHSM_ORG_ID       the organization id an activation key belongs to
#   VM_RHSM_USERNAME     for the credentials mode
#   VM_RHSM_AUTO_ATTACH  attach a subscription after registering
#   VM_RHSM_RELEASE      minor release to pin dnf to, for example 9.7
#
# The key or password itself is never an environment variable: it is staged as a
# 0600 file, read here, and shredded. An env var would sit in
# /etc/vm-bootstrap.env, which is world-readable, and appear in the process list
# of anything that inherited it.
MODE="${VM_RHSM_MODE:-none}"
SECRETS_DIR="${VM_SECRETS_DIR:-/root/vm-bootstrap-secrets}"
SECRET_FILE="$SECRETS_DIR/redhat-subscription-secret"
STATE=/var/lib/vm-bootstrap/subscription
install -d /var/lib/vm-bootstrap

write_state() {
    printf '%s\t%s\t%s\n' "${1:-unknown}" "${2:-}" "${VM_RHSM_RELEASE:-}" > "$STATE"
}

# Nothing to do on a rebuild. The blueprint is RHEL-only, but the shared config
# is not, so check rather than assume.
if [[ "${OS_ID}" != "rhel" ]]; then
    log "not Red Hat Enterprise Linux (this is ${OS_ID}), so no subscription is needed"
    write_state not-applicable "$OS_ID needs no subscription"
    return 0 2>/dev/null || exit 0
fi

if ! have_cmd subscription-manager; then
    warn "subscription-manager is not installed, so this machine cannot be registered"
    write_state failed "subscription-manager missing"
    return 0 2>/dev/null || exit 0
fi

# Already registered, most likely because bootstrap is being re-run. Registering
# twice is an error, and unregistering to re-register would burn an entitlement
# for nothing.
if subscription-manager identity >/dev/null 2>&1; then
    log "already registered with Red Hat"
    write_state registered "already registered"
else
    if [[ "$MODE" == "none" ]]; then
        warn "no Red Hat subscription was configured, so this machine has NO package repositories"
        warn "  everything on the DVD is installed and working, but nothing can be added:"
        warn "  no EPEL, so no xrdp, so no Enhanced Session Mode."
        warn ""
        warn "  to fix it, put an activation key in vm.config.local.yaml:"
        warn "    redHat:"
        warn "      activationKey:  <key>"
        warn "      organizationId: '<org id>'"
        warn "  both are on console.redhat.com under Connector then Activation Keys."
        warn "  then re-run:  sudo vm-bootstrap --only 00-subscription --force"
        write_state unregistered "no subscription configured"
        # Deliberately not a failure. An unregistered RHEL is a legitimate,
        # if limited, machine, and failing here would strand the whole build
        # over something only the operator can supply.
        return 0 2>/dev/null || exit 0
    fi

    if [[ ! -s "$SECRET_FILE" ]]; then
        warn "$MODE was configured but $SECRET_FILE is missing or empty"
        warn "  (shredded by an earlier run? re-render the seed and rebuild)"
        write_state failed "secret missing"
        return 1 2>/dev/null || exit 1
    fi

    attach_args=()
    is_true "${VM_RHSM_AUTO_ATTACH:-true}" && attach_args+=(--auto-attach)

    ok=false
    case "$MODE" in
        activation-key)
            log "registering with Red Hat using an activation key (org ${VM_RHSM_ORG_ID})"
            # The key goes on the command line, which means it is briefly visible
            # in /proc to any local user, the way a password would be. There is
            # no way around it: subscription-manager has no --activationkey-file
            # and does not read one from stdin.
            #
            # Tolerable here, and stated rather than hidden. This runs as root
            # during the first boot, before any interactive login exists, so the
            # window has nobody in it, and an activation key is the credential
            # designed to be revoked on its own. Use a key rather than your
            # account password, and revoke it if the VM is ever shared.
            if subscription-manager register \
                    --org "${VM_RHSM_ORG_ID}" \
                    --activationkey "$(cat "$SECRET_FILE")" \
                    "${attach_args[@]}"; then
                ok=true
            fi
            ;;
        credentials)
            log "registering with Red Hat as ${VM_RHSM_USERNAME}"
            # A password CAN be kept off the command line: omit --password and
            # subscription-manager prompts, which means it reads stdin. Worth
            # doing, because unlike an activation key this is the account login.
            if subscription-manager register \
                    --username "${VM_RHSM_USERNAME}" \
                    "${attach_args[@]}" < "$SECRET_FILE"; then
                ok=true
            fi
            ;;
        *)
            warn "unknown VM_RHSM_MODE '$MODE'"
            write_state failed "unknown mode"
            return 1 2>/dev/null || exit 1
            ;;
    esac

    if ! $ok; then
        warn "registration failed. Common causes:"
        warn "  the key or password is wrong or expired"
        warn "  the organization id does not match the key"
        warn "  no network yet (check: ping -c1 subscription.rhsm.redhat.com)"
        warn "  retry with: sudo vm-bootstrap --only 00-subscription --force"
        write_state failed "registration rejected"
        return 1 2>/dev/null || exit 1
    fi

    log "registered: $(subscription-manager identity 2>/dev/null | head -1)"
    write_state registered "$MODE"
fi

# Pin dnf to the minor release this machine was built as.
#
# Without this the very next step runs a full upgrade and walks the machine onto
# the newest minor Red Hat publishes. Someone who asked for 9.7 asked for 9.7,
# usually because it matches a server they have to reproduce. Clear the pin with
#   sudo subscription-manager release --unset
if [[ -n "${VM_RHSM_RELEASE:-}" ]]; then
    log "pinning dnf to RHEL ${VM_RHSM_RELEASE} so upgrades stay on this minor release"
    subscription-manager release --set="${VM_RHSM_RELEASE}" >/dev/null 2>&1 \
        || warn "could not pin the release to ${VM_RHSM_RELEASE}; upgrades may move to a newer minor"
fi

# The baseline repositories. auto-attach usually enables these already, but say
# so explicitly: a Simple Content Access account attaches nothing, and then the
# repos are off until asked for.
for repo in "rhel-${OS_MAJOR}-for-$(arch)-baseos-rpms" "rhel-${OS_MAJOR}-for-$(arch)-appstream-rpms"; do
    subscription-manager repos --enable "$repo" >/dev/null 2>&1 \
        || warn "could not enable $repo (it may already be on)"
done

# The staged secret has done its job. 55-github.sh shreds the whole directory at
# the end of the run, but that is twenty steps away and this one is the more
# sensitive of the two.
if is_true "${VM_GITHUB_SHRED_SECRETS:-true}" && [[ -f "$SECRET_FILE" ]]; then
    log "shredding the staged Red Hat credential"
    shred -u "$SECRET_FILE" 2>/dev/null || rm -f "$SECRET_FILE"
fi

# Prove there is actually something to install from before 01-base.sh finds out
# the hard way.
if dnf repolist --enabled 2>/dev/null | grep -qE 'rhel-[0-9]+-for-'; then
    log "repositories available:"
    dnf repolist --enabled 2>/dev/null | sed -n '2,8p' | sed 's/^/    /'
else
    warn "no Red Hat repositories are enabled even though registration succeeded"
    warn "  check: subscription-manager status, and subscription-manager repos --list-enabled"
fi
