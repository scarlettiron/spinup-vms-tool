# Databases

Optional. Nothing is installed unless you ask for it.

Two engines are supported for now, PostgreSQL and MySQL, each with a version you
choose. The client and the server are separate switches, because an application VM
usually wants only the client so it can reach a database on another machine, while
the database VM itself needs the server.

## The quick way

Four settings at the top of `vm.config.yaml`:

```powershell
DbEngine  = 'postgres'   # or 'mysql'. Blank installs neither.
DbVersion = ''           # blank takes the engine default
DbClient  = $true        # psql / mysql on the path
DbServer  = $true        # the server too, running as a service
```

or the same in YAML:

```yaml
dbEngine:  postgres
dbVersion: ''
dbClient:  true
dbServer:  true
```

Defaults when `DbVersion` is blank:

| Engine | Default version | Default port | Versions accepted |
| --- | --- | --- | --- |
| `postgres` | 17 | 5432 | 13, 14, 15, 16, 17, 18 |
| `mysql` | 8 | 3306 | 8, 8.0, 8.4 |

`postgresql`, `pgsql` and `pg` are accepted as synonyms for `postgres`, and
`mariadb` for `mysql`. Asking for the server implies the client, so
`DbServer = $true` on its own is enough.

Everything is validated when config loads, so a wrong engine name or an
unsupported version fails during `-RenderOnly` rather than twenty minutes into a
first boot.

## Where the packages come from

This differs per engine, and it is not a style choice.

**PostgreSQL comes from the PostgreSQL project's own repository (PGDG).** Fedora
and Ubuntu each ship whichever major version suited their release, so asking for
17 specifically means using the upstream repo, which publishes for both. If PGDG
has no repository for a very new Fedora yet, the step falls back to the
distribution's packages and says so in the bootstrap report rather than failing
the build.

**MySQL comes from the distribution.** There is no MySQL repository that installs
unattended across both distros without debconf handling, so the version you get is
the one that release ships. The step checks after installing and reports a
mismatch:

```
asked for MySQL 8.4 but this distribution ships 8.0
  MySQL has no unattended cross-distro repository, so the OS package is used
```

That check compares at the precision you asked for: `8` is satisfied by any 8.x,
`8.4` means the 8.4 series specifically. So if you need an exact minor series, pin
one your distribution ships, or use Postgres.

## The rest of the settings

The `Database` section holds what is not in QUICK SETTINGS:

```powershell
Database = @{
    Port          = 0             # 0 takes the engine default
    ListenAddress = 'localhost'   # or '*'
    AdminPassword = ''            # postgres superuser / mysql root
    Databases     = @()           # created empty, owned by the primary user
}
```

## Reaching the database from another VM

By default the server listens on localhost only, and nothing is exposed to the
rest of the virtual switch. To open it up:

```powershell
Database = @{
    ListenAddress = '*'
    AdminPassword = 'something-only-your-team-knows'
}
```

`ListenAddress = '*'` does four things, and all four are needed for a connection
from another machine to actually work:

1. Binds the server to all addresses (`listen_addresses` for Postgres,
   `bind-address` for MySQL).
2. Opens the port in firewalld. Fedora runs firewalld with only ssh and mDNS
   open, so without this the port is closed. Ubuntu ships ufw inactive, so there
   is nothing to open there.
3. Labels the port for SELinux when the port is not the engine's default. This is
   the step that is easy to miss: SELinux labels ports as well as files, so
   Postgres on port 5433 is denied the bind even with the firewall open, and the
   failure looks like the daemon crashing rather than anything mentioning
   SELinux.
4. For Postgres, adds a `pg_hba.conf` rule permitting `scram-sha-256`
   authentication from other hosts. Without it the port is open and every
   connection is still refused. For MySQL, creates the primary user with `'%'` as
   its host so it can connect from anywhere.

A server on `'*'` with a blank `AdminPassword` is refused at build time, because
it would leave the superuser reachable from every machine on the switch.

Then, from another VM or from the Windows host:

```bash
psql "postgresql://dev@acme-fedora-workstation.local:5432/app_dev"
mysql -h acme-fedora-workstation.local -P 3306 -u dev -p
```

These are database protocols, not HTTP, so a browser cannot open them. To test
only whether the port is reachable, from Windows:

```powershell
Test-NetConnection acme-fedora-workstation.local -Port 5432
```

One caveat on `.local` names: mDNS resolution comes from avahi and nss-mdns on the
host OS, so it works from a shell on either VM and from Windows, but generally
**not from inside a Docker container**, which has neither. If your app runs in
Docker or ddev and needs to reach the database VM, use the IP or a Docker network
alias instead of the `.local` name.

## A worked example: the fleet

The natural shape is the client on the application VM and the server on the
database VM, which is what the `Machines` block is for:

```powershell
Machines = @{
    'ubuntu-gui' = @{
        Blueprint = 'ubuntu-desktop'
        OsVersion = '24.04'
        DbEngine  = 'postgres'
        DbClient  = $true          # psql only, to reach the db VM
    }
    'ubuntu-middleware-db' = @{
        Blueprint = 'ubuntu-server'
        OsVersion = '24.04'
        DbEngine  = 'postgres'
        DbServer  = $true
        Database  = @{
            ListenAddress = '*'
            AdminPassword = 'something-only-your-team-knows'
            Databases     = @('app_dev', 'app_test')
        }
    }
}
```

## Adminer

A database browser in a single PHP file:

```powershell
DbEngine      = 'postgres'
DbServer      = $true
DbAdminer     = $true
DbAdminerPort = 3800     # optional, this is the default
```

After the build the route is on the login banner and in the status output:

```
    adminer     http://acme-fedora-workstation.local:3800/   (adminer 4.8.1)
```

Log in with the database credentials. The server field is `localhost` when the
database is on the same VM.

`DbAdminer` needs `DbEngine` set, because Adminer has to be told which driver to
load, and the matching PHP extension is installed for you: `php-pgsql` for
Postgres, `php-mysql` or `php-mysqlnd` for MySQL. It does not need
`DbServer` though. Adminer with only an engine named is a legitimate setup: it
gives you a GUI on this VM for a database running somewhere else.

### Not the Docker image, and how it is served instead

Adminer is served by PHP's own built-in web server behind a systemd unit
(`adminer.service`). That means no nginx, no php-fpm socket path that differs
between Fedora and Ubuntu, and no dependency on Docker being up before the GUI
works.

The trade-off is real and worth stating: PHP's built-in server is single threaded
and explicitly not meant for production traffic. For a single-user database GUI on
a dev VM that is the right call, and it is why this is not offered as anything
other than a dev convenience.

### Who can reach it

The unit binds `0.0.0.0`, so the route works from the Windows host and from the
other VMs on the switch, not only from inside the guest, and the firewall is
opened for its port. That is the point of printing a route at all.

Adminer has no login of its own. It asks for database credentials, so it is only
as protected as the database behind it. On a shared switch, treat the route as
internal: anyone who can reach the port gets a login form against your database.

The port has to differ from the database port, which is checked at build time.

### If it does not come up

The step proves Adminer answers with HTTP 200 before declaring success, so
`running` means a real request succeeded rather than that a package installed.

```bash
systemctl status adminer.service
journalctl -u adminer.service
php -l /usr/share/adminer/index.php
sudo vm-bootstrap --only 36-adminer --force
```

A blank page in the browser usually means the downloaded file was an error page
rather than Adminer. The step runs `php -l` on it for that reason and deletes it
rather than serving something that cannot parse.

## Where the admin password lives

It is never written into the rendered seed as a config value. It is staged as a
single file, `db-admin-password`, mode 0600 and root owned, installed into
`/root/vm-bootstrap-secrets/` in the guest, read once by `35-database.sh`, and
shredded as soon as it has been used.

Two things follow from that. The seed disk does hold it in plain text until first
boot completes, which is why the build warns when it embeds a secret, and the same
caveats as a GitHub token apply. And the password is passed to the database over
stdin or through a temporary 0600 defaults file, never as a command line argument,
so it does not appear in the process list.

If you would rather it never touched the repo at all, keep it in
`vm.config.local.yaml`, which is git-ignored.

## Checking what happened

The bootstrap report lists `35-database` with the rest of the steps, and the login
banner says whether the server is actually answering:

```
database:  postgres 17 running on port 5432
           localhost only
```

From the host:

```powershell
.\scripts\Get-DevVMStatus.ps1
```

```
    database    postgres 17 running on port 5433, reachable from other machines
```

The step proves the server answers a query rather than merely reporting that the
packages installed, so `running` means a connection succeeded. If it installed but
is not answering, that is reported as `FAILED` with the packages still in place,
and the rest of the machine is left alone:

```bash
sudo vm-bootstrap --only 35-database --force
```

## Adding another engine

Two edits. The engine table in [scripts/lib/Common.psm1](../scripts/lib/Common.psm1)
holds the display name, default version and port, accepted versions, where the
packages come from, and the SELinux port type. Then add a case to
[blueprints/bootstrap/35-database.sh](../blueprints/bootstrap/35-database.sh)
that installs and configures it. Nothing else needs to know.
