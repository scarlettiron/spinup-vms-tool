# Optional database engine: client tools, and optionally the server.
#
# Driven entirely by the plan the host resolved, so there are no defaults to
# work out here: VM_DB_ENGINE is either blank or a concrete engine, and the
# version and port are already filled in.
#
#   VM_DB_ENGINE           postgres | mysql | (blank, nothing to do)
#   VM_DB_VERSION          major version, for example 17 or 8
#   VM_DB_SOURCE           pgdg (upstream repo) | distro (the OS packages)
#   VM_DB_PORT             the port to listen on
#   VM_DB_LISTEN_ADDRESS   localhost | *
#   VM_DB_INSTALL_CLIENT   client tools
#   VM_DB_INSTALL_SERVER   the server as well
#   VM_DB_DATABASES        space separated names to create, may be empty
#   VM_DB_SELINUX_TYPE     SELinux port type for this engine
ENGINE="${VM_DB_ENGINE:-}"

if [[ -z "$ENGINE" ]]; then
    log "no database engine requested"
    return 0 2>/dev/null || exit 0
fi

VERSION="${VM_DB_VERSION:-}"
DB_PORT="${VM_DB_PORT:-}"
LISTEN="${VM_DB_LISTEN_ADDRESS:-localhost}"
WANT_SERVER=false
is_true "${VM_DB_INSTALL_SERVER:-false}" && WANT_SERVER=true
SECRETS_DIR="${VM_SECRETS_DIR:-/root/vm-bootstrap-secrets}"
PASSWORD_FILE="$SECRETS_DIR/db-admin-password"
STATE=/var/lib/vm-bootstrap/database
install -d /var/lib/vm-bootstrap

# Set by install_mysql once the root password is known, and used by the
# verification below. Declared here so `set -u` is satisfied on the postgres path.
MYSQL_DEFAULTS=''
MYSQL_ROOT_OPTS='-uroot'

# Recorded for the login banner and for Get-DevVMStatus.ps1, so the state of the
# database is visible without anyone having to know the engine's own commands.
# Written even on failure, which is the case where it matters most.
write_state() {
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$ENGINE" "${1:-unknown}" "${2:-}" "$VERSION" "$DB_PORT" "$LISTEN" > "$STATE"
}

# ---------------------------------------------------------------- PostgreSQL
install_postgres() {
    local pkgs=()

    if [[ "${VM_DB_SOURCE:-}" == "pgdg" ]]; then
        # Each distribution ships whichever major version suited its own
        # release, so the requested version comes from the PostgreSQL project's
        # own repository instead.
        if [[ "$PKG_MANAGER" == "apt" ]]; then
            add_apt_repo pgdg \
                'https://www.postgresql.org/media/keys/ACCC4CF8.asc' \
                "deb [signed-by=/etc/apt/keyrings/pgdg.gpg] https://apt.postgresql.org/pub/repos/apt $(. /etc/os-release && echo "$VERSION_CODENAME")-pgdg main"
            pkgs=("postgresql-client-${VERSION}")
            $WANT_SERVER && pkgs+=("postgresql-${VERSION}")
        else
            # PGDG builds a separate repo rpm per RPM distro, and names them
            # differently: F-42 for Fedora, EL-9 for RHEL and its rebuilds. The
            # RHEL one is keyed on the major version only.
            local pgdg_pkg pgdg_url pgdg_for
            case "$OS_FAMILY" in
                el)
                    pgdg_pkg=pgdg-redhat-repo
                    pgdg_url="https://download.postgresql.org/pub/repos/yum/reporpms/EL-${OS_MAJOR}-x86_64/pgdg-redhat-repo-latest.noarch.rpm"
                    pgdg_for="${OS_ID} ${OS_MAJOR}"
                    ;;
                *)
                    pgdg_pkg=pgdg-fedora-repo
                    pgdg_url="https://download.postgresql.org/pub/repos/yum/reporpms/F-${OS_VERSION_ID}-x86_64/pgdg-fedora-repo-latest.noarch.rpm"
                    pgdg_for="Fedora ${OS_VERSION_ID}"
                    ;;
            esac

            # PGDG lags a new Fedora by a few weeks. Fall back to the distro's
            # own packages rather than failing the whole step.
            if rpm -q "$pgdg_pkg" >/dev/null 2>&1 || \
               dnf install -y "$pgdg_url" >/dev/null 2>&1; then
                # RHEL serves postgresql as an AppStream module, and the module
                # takes priority over the PGDG repo: without this, dnf installs
                # the OS's own version and reports success. Fedora has no
                # modules, so there is nothing to disable there.
                if [[ "$OS_FAMILY" == "el" ]]; then
                    log "disabling the postgresql AppStream module so PGDG's packages win"
                    dnf -qy module disable postgresql >/dev/null 2>&1 || true
                fi
                pkgs=("postgresql${VERSION}")
                $WANT_SERVER && pkgs+=("postgresql${VERSION}-server")
            else
                warn "no PGDG repository for ${pgdg_for}; using the distribution's postgresql packages instead"
                warn "  the installed major version may not be ${VERSION}"
                pkgs=(postgresql)
                $WANT_SERVER && pkgs+=(postgresql-server)
            fi
        fi
    else
        pkgs=(postgresql)
        $WANT_SERVER && pkgs+=(postgresql-server)
    fi

    pkg_install "${pkgs[@]}"
    $WANT_SERVER || return 0

    # Where the server lives differs by packaging. PGDG on RPM keeps versioned
    # paths and a versioned unit; everything else uses the plain names.
    local unit=postgresql bindir=/usr/bin datadir=''
    if [[ "$PKG_MANAGER" != "apt" ]] && [[ -d "/usr/pgsql-${VERSION}" ]]; then
        unit="postgresql-${VERSION}"
        bindir="/usr/pgsql-${VERSION}/bin"
        datadir="/var/lib/pgsql/${VERSION}/data"
    elif [[ "$PKG_MANAGER" != "apt" ]]; then
        datadir=/var/lib/pgsql/data
    fi

    # Debian initialises a cluster from the package; RPM does not.
    if [[ -n "$datadir" && ! -f "$datadir/PG_VERSION" ]]; then
        log "initialising the PostgreSQL data directory"
        if [[ -x "$bindir/postgresql-${VERSION}-setup" ]]; then
            "$bindir/postgresql-${VERSION}-setup" initdb
        elif have_cmd postgresql-setup; then
            postgresql-setup --initdb
        else
            sudo -u postgres "$bindir/initdb" -D "$datadir"
        fi
    fi

    systemctl enable --now "$unit"

    # ALTER SYSTEM writes postgresql.auto.conf, which spares us having to find
    # postgresql.conf on three different packaging layouts.
    log "setting listen_addresses and port"
    sudo -u postgres psql -c "ALTER SYSTEM SET listen_addresses = '${LISTEN}';" >/dev/null
    sudo -u postgres psql -c "ALTER SYSTEM SET port = ${DB_PORT};" >/dev/null

    if [[ -s "$PASSWORD_FILE" ]]; then
        log "setting the postgres superuser password"
        # Read here as root: the file is 0600 root-owned, so anything running as
        # the postgres user cannot read it. Piped over stdin rather than passed as
        # an argument, so it stays out of the process list, and single quotes are
        # doubled so a password containing one cannot break out of the literal.
        local pw
        pw="$(cat "$PASSWORD_FILE")"
        printf "ALTER USER postgres WITH PASSWORD '%s';\n" "${pw//\'/\'\'}" \
            | sudo -u postgres psql -v ON_ERROR_STOP=1 >/dev/null
    fi

    # A remote listener needs a matching pg_hba rule; without one the port is
    # open and every connection is still refused.
    if [[ "$LISTEN" == "*" ]]; then
        local hba
        hba="$(sudo -u postgres psql -Atc 'SHOW hba_file;')"
        if [[ -n "$hba" ]] && ! grep -q 'vm-bootstrap' "$hba" 2>/dev/null; then
            log "allowing password authentication from other machines in pg_hba.conf"
            {
                echo ''
                echo '# added by vm-bootstrap: Database.ListenAddress is *'
                echo 'host    all             all             0.0.0.0/0               scram-sha-256'
                echo 'host    all             all             ::/0                    scram-sha-256'
            } >> "$hba"
        fi
    fi

    systemctl restart "$unit"

    local db
    for db in ${VM_DB_DATABASES:-}; do
        if sudo -u postgres psql -p "$DB_PORT" -Atc \
            "SELECT 1 FROM pg_database WHERE datname = '${db}';" | grep -q 1; then
            log "database $db already exists"
        else
            log "creating database $db owned by ${VM_PRIMARY_USER}"
            sudo -u postgres createuser -p "$DB_PORT" "$VM_PRIMARY_USER" 2>/dev/null || true
            sudo -u postgres createdb -p "$DB_PORT" -O "$VM_PRIMARY_USER" "$db"
        fi
    done
}

# --------------------------------------------------------------------- MySQL
install_mysql() {
    local pkgs=()
    if [[ "$PKG_MANAGER" == "apt" ]]; then
        pkgs=(mysql-client)
        $WANT_SERVER && pkgs+=(mysql-server)
    else
        pkgs=(mysql)
        $WANT_SERVER && pkgs+=(mysql-server)
    fi
    pkg_install "${pkgs[@]}"

    # MySQL comes from the distribution, so the major version is whatever this
    # release ships. Say so plainly rather than letting it pass as the version
    # that was asked for.
    local installed want
    installed="$(mysql --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -1)"
    if [[ -n "$installed" && -n "$VERSION" ]]; then
        # Compare at the precision that was asked for: '8' is happy with any 8.x,
        # but '8.4' means the 8.4 series and 8.0 is not it.
        if [[ "$VERSION" == *.* ]]; then
            want="$VERSION"
        else
            want="${VERSION}"
            installed="${installed%%.*}"
        fi
        if [[ "$installed" != "$want" ]]; then
            warn "asked for MySQL ${VERSION} but this distribution ships ${installed}"
            warn "  MySQL has no unattended cross-distro repository, so the OS package is used"
            warn "  pin a version this release ships, or use postgres, where the upstream repo is used"
        else
            log "MySQL ${installed} installed"
        fi
    fi

    $WANT_SERVER || return 0

    local unit=mysql confdir=/etc/mysql/mysql.conf.d
    if [[ "$PKG_MANAGER" != "apt" ]]; then
        unit=mysqld
        confdir=/etc/my.cnf.d
    fi
    [[ -d "$confdir" ]] || confdir=/etc/mysql/conf.d
    install -d "$confdir"

    local bind=127.0.0.1
    [[ "$LISTEN" == "*" ]] && bind=0.0.0.0

    log "writing $confdir/zz-vm-bootstrap.cnf"
    cat > "$confdir/zz-vm-bootstrap.cnf" <<CNF
# GENERATED by vm-bootstrap. Edit Database.* in vm.config.yaml, not this file.
[mysqld]
port = ${DB_PORT}
bind-address = ${bind}
CNF
    chmod 0644 "$confdir/zz-vm-bootstrap.cnf"
    selinux_restore "$confdir"

    systemctl enable --now "$unit"
    systemctl restart "$unit"

    # mysql on the socket as root works without a password on a fresh install,
    # which is how the password gets set in the first place. Afterwards it needs
    # the password, supplied through a 0600 defaults file rather than -p on the
    # command line, which would put it in the process list for every local user
    # to read. MYSQL_ROOT_OPTS is set once and reused by everything below.
    if [[ -s "$PASSWORD_FILE" ]]; then
        log "setting the MySQL root password"
        local pw escaped
        pw="$(cat "$PASSWORD_FILE")"
        escaped="${pw//\'/\'\'}"
        mysql --protocol=socket -uroot <<SQL || warn "could not set the MySQL root password; it may already be set"
ALTER USER 'root'@'localhost' IDENTIFIED WITH caching_sha2_password BY '${escaped}';
FLUSH PRIVILEGES;
SQL
        MYSQL_DEFAULTS="$(mktemp /root/.my.vm-bootstrap.XXXXXX)"
        chmod 0600 "$MYSQL_DEFAULTS"
        printf '[client]\nuser=root\npassword=%s\n' "$pw" > "$MYSQL_DEFAULTS"
        MYSQL_ROOT_OPTS="--defaults-extra-file=$MYSQL_DEFAULTS -uroot"

        if [[ "$LISTEN" == "*" ]]; then
            log "creating ${VM_PRIMARY_USER} for connections from other machines"
            mysql $MYSQL_ROOT_OPTS --protocol=socket <<SQL || warn "could not create the remote user"
CREATE USER IF NOT EXISTS '${VM_PRIMARY_USER}'@'%' IDENTIFIED BY '${escaped}';
GRANT ALL PRIVILEGES ON *.* TO '${VM_PRIMARY_USER}'@'%' WITH GRANT OPTION;
FLUSH PRIVILEGES;
SQL
        fi
    fi

    local db
    for db in ${VM_DB_DATABASES:-}; do
        log "creating database $db"
        mysql $MYSQL_ROOT_OPTS --protocol=socket \
            -e "CREATE DATABASE IF NOT EXISTS \`${db}\`;" || warn "could not create $db"
    done
}

# ----------------------------------------------------------------------- run
WHAT='client tools'
$WANT_SERVER && WHAT='client tools + server'
log "database: $ENGINE $VERSION ($WHAT)  port $DB_PORT  listen $LISTEN"

# The port has to be permitted before the server tries to bind it, or the start
# below fails for a reason that looks nothing like a firewall or SELinux problem.
if $WANT_SERVER; then
    selinux_allow_port "${VM_DB_SELINUX_TYPE:-}" "$DB_PORT"
    if [[ "$LISTEN" == "*" ]]; then
        firewall_allow_port "$DB_PORT"
    fi
fi

case "$ENGINE" in
    postgres) install_postgres ;;
    mysql)    install_mysql ;;
    *)
        warn "unknown VM_DB_ENGINE '$ENGINE'"
        write_state failed "unknown engine"
        return 1 2>/dev/null || exit 1
        ;;
esac

# --------------------------------------------------------------- verification
if ! $WANT_SERVER; then
    log "client tools installed; no server was requested"
    write_state client-only "client tools only"
else
    # Prove it is actually accepting connections rather than merely installed.
    ok=false
    case "$ENGINE" in
        postgres)
            sudo -u postgres psql -p "$DB_PORT" -Atc 'SELECT version();' >/dev/null 2>&1 && ok=true
            ;;
        mysql)
            mysql $MYSQL_ROOT_OPTS --protocol=socket -e 'SELECT VERSION();' >/dev/null 2>&1 && ok=true
            ;;
    esac

    if $ok; then
        log "$ENGINE is running and accepting connections on port $DB_PORT"
        if [[ "$LISTEN" == "*" ]]; then
            log "reachable from other machines as $(hostnamectl --static):${DB_PORT}"
        else
            log "listening on localhost only; set Database.ListenAddress to '*' to open it up"
        fi
        write_state running "port $DB_PORT"
    else
        warn "$ENGINE was installed but is not answering on port $DB_PORT"
        warn "  check: systemctl status, and journalctl -u the service unit"
        write_state failed "installed but not answering"
        # Not a hard failure: the packages are there and the rest of the machine
        # is fine. The report and the banner both carry the warning.
    fi
fi

# The password has served its purpose. Shred it here rather than relying on
# 55-github.sh clearing the whole directory later, and take the temporary MySQL
# defaults file with it.
for secret in "$PASSWORD_FILE" "$MYSQL_DEFAULTS"; do
    [[ -n "$secret" && -f "$secret" ]] || continue
    shred -u "$secret" 2>/dev/null || rm -f "$secret"
done
