# Scarlett A. Scott
# Created 2026-09-15

# Create the additional accounts listed in Guest.AdditionalUsers.
# Format of /etc/vm-bootstrap.users (tab separated, one line per user):
#   username <TAB> realname <TAB> sha512-hash <TAB> sudo(0|1) <TAB> extra,groups <TAB> base64(authorized_keys)
USERS_FILE=/etc/vm-bootstrap.users

if [[ ! -s "$USERS_FILE" ]]; then
    log "no additional users requested"
    return 0 2>/dev/null || exit 0
fi

while IFS=$'\t' read -r username realname hash sudo_flag groups keys_b64; do
    [[ -n "${username:-}" ]] || continue
    [[ "${username:0:1}" == "#" ]] && continue

    if id -u "$username" >/dev/null 2>&1; then
        log "user $username already exists"
    else
        log "creating user $username"
        useradd --create-home --shell /bin/bash --comment "$realname" "$username"
    fi

    if [[ -n "${hash:-}" && "$hash" != "-" ]]; then
        printf '%s:%s\n' "$username" "$hash" | chpasswd -e
    fi

    if is_true "${sudo_flag:-0}"; then
        usermod -aG "$(admin_group)" "$username"
    fi

    if [[ -n "${groups:-}" && "$groups" != "-" ]]; then
        IFS=',' read -ra extra_groups <<< "$groups"
        for g in "${extra_groups[@]}"; do
            [[ -n "$g" ]] || continue
            getent group "$g" >/dev/null || groupadd "$g"
            usermod -aG "$g" "$username"
        done
    fi

    if [[ -n "${keys_b64:-}" && "$keys_b64" != "-" ]]; then
        home="$(getent passwd "$username" | cut -d: -f6)"
        install -d -m 0700 -o "$username" -g "$username" "$home/.ssh"
        printf '%s' "$keys_b64" | base64 -d > "$home/.ssh/authorized_keys"
        chown "$username:$username" "$home/.ssh/authorized_keys"
        chmod 0600 "$home/.ssh/authorized_keys"
    fi
done < "$USERS_FILE"
