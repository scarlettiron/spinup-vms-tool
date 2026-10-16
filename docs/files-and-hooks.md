# Copying files in, and post-start hooks

Two related things, both optional and both driven from config.

- **Files** carries directories from this laptop into the guest.
- **Hooks** runs your own commands in the guest once everything else is
  provisioned.

## Copying files in

The single-directory case is two quick settings:

```yaml
copyFrom: files         # a directory on this laptop
copyTo:   ''            # blank means the primary user's home
```

The *contents* of `files/` are copied, not the directory itself. A relative path
is relative to the project folder (the one holding `.vmseed/`), so a committed
`files/` directory works the same for everyone on the team.

For more than one destination, use the list:

```yaml
files:
  copy:
    - { from: files/home,  to: '~' }
    - { from: files/certs, to: /usr/local/share/ca-certificates, owner: root, mode: '0644' }
```

| Key | Meaning |
| --- | --- |
| `from` | A directory on this laptop. Relative paths are relative to the project folder. |
| `to` | Where the contents land. Absolute, or starting with `~` for the primary user's home. Blank means `~`. |
| `owner` | `user` or `user:group`. Blank means the primary user. |
| `mode` | Octal, applied to every file. Blank means `0644`, except `.sh` files which get `0755`. Quote it in YAML (`'0644'`). |

File modes on this computer are not carried over (Windows has none to carry). Shell
scripts are the one case worth inferring; anything else that needs to be
executable wants an explicit `mode`.

Directory structure is preserved, and files are copied byte for byte, binaries
included: they travel base64-encoded inside the seed, so nothing is reinterpreted
as text and no line ending is rewritten.

### The size limit, and why there is one

Everything here is embedded in the seed disk, which is a 256 MB volume, and base64
inflates it by a third. So:

- over **10 MB** warns that the install will be noticeably slower
- over **64 MB** fails the build

This is for configuration, certificates, licence files and small fixtures. For
bulk data, use a data disk or a share.

### How it actually gets there

The seed stages each directory under `/var/lib/vm-bootstrap/files/<n>/` with its
layout intact, and writes a manifest to `/etc/vm-bootstrap.files`. Step
`06-files.sh` reads that manifest and moves each tree into place.

That indirection is the point: `~` cannot be resolved on the host. Where a user's
home actually is, and which uid owns it, are things only the guest knows. The step
runs after `05-users`, so an account named as an `owner` already exists by then.

Only the files that came from the staging tree are re-owned, so copying into a
shared destination like `/etc` does not take ownership of everything already
there.

## Post-start hooks

Commands to run in the guest once everything else is provisioned:

```yaml
postStart:
  - ddev config global --no-bind-mounts
  - cd ~/src/web && npm ci
```

They are rendered into a single script, installed at
`/usr/local/lib/vm-bootstrap/hooks/post-start.sh`, and run by step `80-hooks`.
That position matters: it is after `70-repos`, so a hook can work inside a clone,
and before `99-finalize`, so its result reaches the bootstrap report and the login
banner.

The full set of options:

```yaml
hooks:
  postStart:        []        # commands, as above (also a quick setting)
  postStartFile:    ''        # a script on this laptop, appended after them
  runAs:            user      # user or root
  workingDirectory: ''        # absolute or '~' relative; blank means that user's home
  runAtEveryBoot:   false
```

`runAs: user` is the default and runs the script through a login shell as the
primary user, so `nvm`, `ddev` and anything else on their `PATH` is present
exactly as it would be if they had logged in and typed the command. Use `root`
for system changes.

A single command is fine in `postStart`; so is a multi-line string, which is the
natural way to write anything longer:

```yaml
postStart:
  - |
    cd ~/src/web
    npm ci
    npm run build
```

`postStartFile` is for anything long enough to deserve its own file under version
control. Its contents are appended after the inline commands, with a leading
shebang stripped since it is being appended rather than launched.

### Failure

The generated script runs with `set -euo pipefail`, so the first failing command
stops it and the step is reported as failed. That does not strand the machine:
every other step has already run. Fix the config and re-run just this step:

```bash
sudo vm-bootstrap --only 80-hooks --force
```

Output goes to `/var/log/vm-post-start.log` as well as the bootstrap log, so a
hook that prints a lot does not have to be dug out of the middle of the install.

### Running on every boot

```yaml
hooks:
  runAtEveryBoot: true
```

This installs `vm-post-start.service` as well, so the same script runs on every
start rather than only at the end of provisioning. It is enabled with `--now`, so
the first-boot run happens through the unit rather than twice.

Make the commands idempotent if you set this. `mkdir -p` and `ddev config` are
fine; anything that appends to a file is not.

```bash
systemctl status vm-post-start.service
journalctl -u vm-post-start.service
```

## Checking what happened

Both show up in the bootstrap report alongside every other step, and in the
status:

```sh
vmseed status
```

```
    hooks       runs on every boot, 14 line(s) in /var/log/vm-post-start.log
```

Before a machine is up, both are reported from config instead, so you can see what
a build will do without building it:

```sh
vmseed create --render
```

```
      files          5 file(s), 98 KB, from 2 folder(s)
                     ~/work/acme/files/home to ~ (owner dev)
                     ~/work/acme/files/certs to /usr/local/share/ca-certificates (owner root)
      hooks          2 command(s), as user, once, after provisioning
```

`--render` also writes the rendered seed to the VM's `build/` folder
(`~/.vmseed/vms/<name>/build/` by default). The generated hook script is in it,
installed at `/usr/local/lib/vm-bootstrap/hooks/post-start.sh`, and is worth
reading before a real build: it is exactly what the guest will run.
