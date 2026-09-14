# Tidy up, record what we built, and reboot into the desktop if asked.
pkg_cleanup

BANNER=/etc/vm-bootstrap.summary
USER_HOME="$(getent passwd "${VM_PRIMARY_USER:-root}" | cut -d: -f6)"
PUBKEY="$USER_HOME/.ssh/id_${VM_GITHUB_SSH_KEY_TYPE:-ed25519}.pub"

{
    echo "Provisioned by ubuntu-dev-vm on $(date -Is)"
    echo "blueprint: ${VM_BLUEPRINT:-unknown}"
    echo "hostname:  $(hostnamectl --static)"
    echo "user:      ${VM_PRIMARY_USER:-unknown}"
    if is_true "${VM_ENABLE_MDNS:-true}"; then
        echo "mdns name: $(hostnamectl --static).local"
    fi
    echo "ip:        $(hostname -I 2>/dev/null | tr -s ' ')"
    echo "log:       /var/log/vm-bootstrap.log"
    echo "report:    /var/log/vm-bootstrap-report.txt"

    if [[ -s /etc/vm-bootstrap.disks ]] && grep -qvE '^\s*(#|$)' /etc/vm-bootstrap.disks; then
        echo
        echo "disks:"
        while IFS=$'\t' read -r label _lun mountpoint _fs _size; do
            [[ -n "${label:-}" && "${label:0:1}" != "#" ]] || continue
            printf '  %-10s %-16s %s\n' "$label" "$mountpoint" \
                "$(df -h --output=size,used,avail "$mountpoint" 2>/dev/null | tail -1 | tr -s ' ' || echo 'not mounted')"
        done < /etc/vm-bootstrap.disks
    fi

    # What 35-database.sh ended up with, so someone logging in can see whether
    # the database is actually up and on which port, without knowing the engine's
    # own commands.
    if [[ -s /var/lib/vm-bootstrap/database ]]; then
        IFS=$'\t' read -r db_engine db_status db_detail db_version db_port db_listen \
            < /var/lib/vm-bootstrap/database
        echo
        case "$db_status" in
            running)
                echo "database:  ${db_engine} ${db_version} running on port ${db_port}"
                if [[ "$db_listen" == "*" ]]; then
                    echo "           reachable from other machines on this switch"
                else
                    echo "           localhost only"
                fi
                ;;
            client-only) echo "database:  ${db_engine} ${db_version} client tools only, no server" ;;
            failed)      echo "database:  ${db_engine} ${db_version} FAILED - ${db_detail}" ;;
            *)           echo "database:  ${db_engine} ${db_version} ${db_status} ${db_detail}" ;;
        esac
    fi

    # The Adminer route, which is the whole point of installing it: the first thing
    # someone wants after a build is the URL to paste into a browser.
    if [[ -s /var/lib/vm-bootstrap/adminer ]]; then
        IFS=$'\t' read -r adm_status adm_detail adm_version adm_port \
            < /var/lib/vm-bootstrap/adminer
        echo
        case "$adm_status" in
            running) echo "adminer:   ${adm_detail}" ;;
            failed)  echo "adminer:   FAILED - ${adm_detail}" ;;
            *)       echo "adminer:   ${adm_status} ${adm_detail}" ;;
        esac
    fi

    if [[ -f /var/log/vm-post-start.log ]]; then
        echo
        echo "hooks:     ran, output in /var/log/vm-post-start.log"
    fi

    # What 55-github.sh found when it ran `ssh -T git@github.com`, so the first
    # thing you see on login is whether GitHub access actually works.
    if [[ -s /var/lib/vm-bootstrap/github-auth ]]; then
        IFS=$'\t' read -r gh_status gh_detail < /var/lib/vm-bootstrap/github-auth
        echo
        case "$gh_status" in
            ok)          echo "github:    authenticated as $gh_detail" ;;
            mismatch)    echo "github:    authenticated as $gh_detail - NOT the account you configured" ;;
            denied)      echo "github:    NOT authenticated - $gh_detail" ;;
            unknown)     echo "github:    unverified - $gh_detail" ;;
            unreachable) echo "github:    unverified - $gh_detail" ;;
            *)           echo "github:    $gh_status $gh_detail" ;;
        esac
    fi

    # The public half of the key, so a fresh VM tells you what to paste into
    # https://github.com/settings/keys the moment you log in. Only worth showing
    # when GitHub has not already accepted it.
    if [[ -f "$PUBKEY" ]] && ! grep -q '^ok' /var/lib/vm-bootstrap/github-auth 2>/dev/null; then
        echo
        echo "github ssh key (add at https://github.com/settings/keys):"
        echo "  $(cat "$PUBKEY")"
    fi
} > "$BANNER"
cat "$BANNER"

# Greet anyone who logs in with what this machine actually is.
cat "$BANNER" > /etc/motd

if is_true "${VM_REBOOT_WHEN_DONE:-true}"; then
    log "rebooting into the desktop in 10s"
    # Detached so the reboot cannot kill the runner mid-write and leave the
    # step un-stamped, which would make bootstrap run all over again.
    systemd-run --on-active=10 --unit=vm-bootstrap-reboot systemctl reboot >/dev/null 2>&1 \
        || (sleep 10 && systemctl reboot) &
fi
