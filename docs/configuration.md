# Configuration

Two layers of YAML in the project's `.vmseed/` folder. The team standard is
committed; your own overrides are not.

| Layer | File | Committed? |
| --- | --- | --- |
| team standard | `.vmseed/vm.config.yaml` | yes |
| per-developer | `.vmseed/vm.config.local.yaml` | no, git-ignored |

`vmseed init` writes both, plus `vm.config.example.yaml` and
`vm.config.local.example.yaml` beside them with every setting documented. To
reuse config files from another project, use `vmseed init --import <folder>`.

Your local file only names what you want to differ. Everything else falls through
to `vm.config.yaml`. `vmseed config` shows the merged result, and
`vmseed config --validate` checks it.

## Reading the config file

Every section carries a tag saying whether it is meant to be edited:

| Tag | Means |
| --- | --- |
| `@user-editable` | change these freely; that is what they are there for |
| `@advanced` | the default is right for almost everyone, and the comment next to it says what changing it costs |

Grep for the tag when you are looking for somewhere to change something:

```sh
grep -n '@user-editable' .vmseed/vm.config.example.yaml
```

`@user-editable` covers: the quick settings block, `vm.dataDisks`,
`guest.primaryUser`, `guest.additionalUsers`, `bootstrap`, `git`, `github`
(`sshKey`, `cli`, `uploadSshKey`), and `machines`.

`@advanced` covers the rest of `vm`, `guest.ssh`, `github.knownHosts`,
`github.shredSecretsAfterUse`, and `iso`. Nothing there is off-limits, it just
has a default that is right unless you have a specific reason.

Blueprints are a level below all of this: they describe what an OS
*is* rather than how your VM is configured. See
[adding-an-os.md](adding-an-os.md).

## Quick settings

The handful of settings people actually change are available as flat names, so
they can sit in one block at the top of the file instead of nested four levels
deep. Each is a shortcut for exactly one real setting:

| Quick setting | Is really | |
| --- | --- | --- |
| `projectName` | (top level) | names the project; VM and hostname default to `<projectName>-<blueprint>` |
| `vmName` | `vm.name` | overrides the derived VM name |
| `vmPath` | `vm.path` | where the VM and its disks live |
| `cpus` | `vm.cpuCount` | |
| `memoryGB` | `vm.memoryStartupGB` | |
| `maxMemoryGB` | `vm.memoryMaximumGB` | dynamic-memory ceiling |
| `diskGB` | `vm.diskSizeGB` | the **system** disk; others are `vm.dataDisks` |
| `switch` | `vm.switchName` | |
| `enhancedSession` | `vm.enhancedSession` | clipboard + resizable desktop |
| `hostname` | `guest.hostname` | overrides the derived hostname; also `<hostname>.local` |
| `username` | `guest.primaryUser.username` | |
| `realName` | `guest.primaryUser.realName` | |
| `password` | `guest.primaryUser.password` | hashed at build time |
| `passwordHash` | `guest.primaryUser.passwordHash` | supply your own crypt hash |
| `timezone` | `guest.timezone` | |
| `locale` | `guest.locale` | |
| `keyboardLayout` | `guest.keyboardLayout` | |
| `gitName` | `git.userName` | |
| `gitEmail` | `git.userEmail` | |
| `githubUsername` | `github.username` | your GitHub account name |
| `githubToken` | `github.cli.token` | put this in the local file only |
| `githubTokenFile` | `github.cli.tokenFile` | a path on this laptop |
| `sshKeyMode` | `github.sshKey.mode` | `auto` / `generate` / `copy` / `none` |
| `repos` | `repos.items` | when given a list |
| `reposRoot` | `repos.root` | |
| `osVersion` | `iso.release` | the OS release; blank uses the blueprint's `os.version` |
| `release` | `iso.release` | synonym for `osVersion` |
| `ubuntuRelease` | `iso.release` | synonym, kept for older configs |
| `isoPath` | `iso.path` | use a local .iso |
| `installDesktop` | `bootstrap.installDesktop` | |
| `installDocker` | `bootstrap.installDocker` | |
| `installDdev` | `bootstrap.installDdev` | |
| `installNode` | `bootstrap.installNode` | |
| `installPython` | `bootstrap.installPython` | |
| `installVSCode` | `bootstrap.installVSCode` | |
| `installChrome` | `bootstrap.installChrome` | Google Chrome; skipped without a desktop |
| `installRemoteAccess` | `bootstrap.installRemoteAccess` | VPN, RDP and SSH tunnelling clients; GUI parts need a desktop |
| `nodeVersion` | `bootstrap.nodeVersion` | any nvm alias |
| `resolution` | `bootstrap.displayResolution` | |
| `extraPackages` | `bootstrap.extraAptPackages` | |
| `dotfilesRepo` | `bootstrap.dotfilesRepo` | |
| `enableMdns` | `bootstrap.enableMdns` | |
| `rebootWhenDone` | `bootstrap.rebootWhenDone` | |
| `dbEngine` | `database.engine` | `postgres` / `mysql`; blank installs neither |
| `dbVersion` | `database.version` | blank takes the engine default: postgres 17, mysql 8 |
| `dbClient` | `database.installClient` | the command line client only |
| `dbServer` | `database.installServer` | the server too; implies the client |
| `dbPort` | `database.port` | 0 takes the engine default: 5432 / 3306 |
| `dbListenAddress` | `database.listenAddress` | `localhost` or `*` |
| `dbAdminPassword` | `database.adminPassword` | put this in the local file only |
| `dbDatabases` | `database.databases` | names to create on first boot |
| `dbAdminer` | `database.adminer.install` | the single-file database browser; needs `dbEngine` |
| `dbAdminerPort` | `database.adminer.port` | where Adminer is served, default 3800 |
| `copyFrom` | `files.source` | a local directory whose contents are copied into the guest |
| `copyTo` | `files.destination` | where they land; blank means the primary user's home |
| `postStart` | `hooks.postStart` | commands to run once the VM is provisioned |
| `postStartFile` | `hooks.postStartFile` | a local script, appended after those commands |
| `postStartRunAs` | `hooks.runAs` | `user` (default) or `root` |
| `postStartEveryBoot` | `hooks.runAtEveryBoot` | also run on every subsequent boot |

Anything without a shortcut is written out in full. Both styles can appear in the
same file - within one file the shortcut wins:

```yaml
cpus: 8                   # shortcut
vm:
  dataDisks:              # long-hand, no shortcut for this
    - label: drive-e
      driveLetter: E
      sizeGB: 80
```

Shortcuts work inside a `machines` block too.

**Names are case-insensitive.** `cpus`, `Cpus`, and `CPUS` are the same setting.
The example files use camelCase throughout.

**Typos are rejected.** An unrecognised top-level name fails the build with a
suggestion, rather than being silently ignored:

```
error: unknown setting 'cpuz' in .vmseed/vm.config.local.yaml. Did you mean 'cpus'?
Every setting is listed in vm.config.example.yaml beside it.
```

## Naming, and the address you browse to

`projectName` names the work rather than the OS. The VM and the guest hostname
both default to `<projectName>-<blueprint>`:

```yaml
projectName: acme
blueprint:   fedora-workstation
#  VM name and hostname become: acme-fedora-workstation
```

`bootstrap.enableMdns` publishes that hostname with avahi, so it is also the
address a browser uses, with no hosts file and no static IP:

```
http://acme-fedora-workstation.local
```

Change `projectName` and every machine in the project is renamed together.

**Precedence, highest first:** an explicit `vmName` / `hostname`, then a
`machines` key (a machine is named after its key), then
`<projectName>-<blueprint>`.

The derived name is lower-cased, and both it and an explicit `hostname` are
checked as DNS labels, because a name that is not one will not resolve over mDNS
and the address would be useless:

- letters, digits and hyphens only, not starting or ending with a hyphen
- 63 characters maximum

`projectName` is checked the same way, and rejected if it would produce an
invalid label. All of these fail at render time, not after a 30-minute build.

### The name-is-taken check

A real build refuses to start if the name is already in use, which catches two
different collisions:

| Clash | Detected by | Result |
| --- | --- | --- |
| a VM of that name exists | the hypervisor | fails; `vmseed create --force` replaces it |
| `<hostname>.local` already answers | the OS resolver, which does mDNS | fails |

The second matters because two machines claiming one mDNS name means avahi
renames the loser to `<hostname>-2.local`, so the address you hand a colleague
quietly reaches the wrong machine. It is not a hypervisor question: the name might
belong to a teammate's laptop on the same network.

`--force` skips the hostname check, since the thing answering is almost certainly
the VM being replaced.

`create --render` skips both checks, since nothing is being created.
`create --all` also refuses two machines that resolve to the same VM name or the
same hostname.

## Choosing the OS and its version

`blueprint` and `osVersion` sit together at the top of the config because they are
coupled: `43` is a Fedora release and means nothing to Ubuntu, `24.04` is an
Ubuntu release and means nothing to Fedora.

| Blueprint | `osVersion` looks like | Support |
| --- | --- | --- |
| `fedora-workstation` | `43` | about 13 months per release. **Fedora has no LTS**, and a retired release is moved off the mirrors, so a stale value fails on a 404. |
| `redhat-rocky-workstation` | `9` | RHEL 9 lifecycle: full support to 2027, maintenance to May 2032. |
| `redhat-rhel-workstation` | `9.7` | Same lifecycle. A full minor release, because the ISO is a specific file you supply. |
| `ubuntu-desktop` | `24.04` | LTS: 5 years, to April 2029. `26.04` is the newer LTS. |
| `ubuntu-server` | `24.04` | same as `ubuntu-desktop` |

`redhat-rocky-workstation` installs Rocky Linux, a free binary-compatible rebuild
of Red Hat Enterprise Linux. Red Hat's own ISOs are behind a subscription and
cannot be fetched or verified by this repo, so Rocky is what the blueprint builds;
it is `9` only, because EPEL 10 does not yet carry `xrdp` and without that there
is no Enhanced Session Mode.

`redhat-rhel-workstation` installs Red Hat's own build. It is the only blueprint
that cannot build from a clean checkout, for two commercial reasons: you must
supply the binary DVD yourself through `isoPath`, and you must give it a
subscription in the `redHat` section or the VM ends up with no package
repositories at all. See [the Red Hat section](#red-hat-subscription) below.

Each blueprint declares the shape its versions take, and a mismatch fails at
render time rather than 404ing on a download:

```
error: osVersion '43' does not look like a ubuntu-desktop release. Its own version
is 24.04. Set osVersion to match the blueprint, or remove it to use the
blueprint's own.
```

**Two ways to run this, and the trade-off is real:**

- **Name the version explicitly** (what ships). Clear at a glance, but every
  `machines` entry on a different blueprint has to name its own version too,
  which is why both Ubuntu machines carry `osVersion: '24.04'`.
- **Leave `osVersion` blank.** Each blueprint then supplies its own
  (`os.version`), so a mixed fleet needs no version keys at all and switching OS
  is genuinely one line. Less explicit, fewer places to keep in step.

To pin a release for the long term, set it in the blueprint's `os.version`, not in
config: that way it travels with the OS it belongs to.

## Precedence

Lowest to highest. Set a key in whichever layer it belongs to; the rest is
inherited:

| | Layer | Purpose |
| --- | --- | --- |
| 0 | built in | Vmseed's own defaults, some worked out for this computer (paths, time zone) |
| 1 | the blueprint's `defaults` | what the OS *suggests* - sizing, nice-to-haves |
| 2 | `vm.config.yaml` | the team standard |
| 3 | the blueprint's `overrides` | what the OS *is* - a server blueprint has no desktop |
| 4 | `vm.config.local.yaml` | per-developer |
| 5 | `machines.<name>` | the per-machine block, when a command names that machine (`vmseed create ubuntu-gui`) |

Layers 1 and 3 both come from the selected blueprint and deliberately sit on
opposite sides of the team standard - see
[adding-an-os.md](adding-an-os.md#inheriting).

Quick settings are expanded **per layer, before merging**, so precedence reads
the way you would expect: `cpus: 8` in your local file beats
`vm: { cpuCount: 6 }` in the shared file, and vice versa.

Nested settings merge key by key. Lists replace wholesale, so an override can
shorten a package list rather than only ever growing it.

## Seeing the result

```sh
vmseed config              # the merged config, every layer applied
vmseed create --render     # the plan, plus the rendered seed files
```

`create --render` prints the resolved plan and writes the fully rendered seed
(cloud-init or kickstart) plus every generated bootstrap file to the VM folder's
`build/` (by default `~/.vmseed/vms/<vm-name>/build/`). No hypervisor changes -
the fastest way to check a config edit did what you meant.

## YAML notes

Config files are ordinary YAML. A few things worth knowing:

- Quote a release number: `24.04` unquoted is the number 24.04, not the string.
- On/off settings also accept `yes`, `no`, `on`, `off` and `1`/`0`. Quote them
  (`'no'`) where you mean the string.
- Values that look alarming are fine unquoted, because a colon only separates a
  key when followed by whitespace: `git@github.com:acme/web.git`, `lts/*`,
  `en_US.UTF-8`, `1920x1080`. A Windows path with backslashes is fine unquoted
  or in single quotes, but not in double quotes, where `\` starts an escape.

## Where the rest is documented

- [disks.md](disks.md) - the three-disk layout, growing and moving disks
- [github-access.md](github-access.md) - SSH keys, tokens, `ssh_keys/`
- [repos.md](repos.md) - cloning repositories
- [databases.md](databases.md) - postgres or mysql, client and server, Adminer
- [files-and-hooks.md](files-and-hooks.md) - copying files in, post-start hooks
- [adding-an-os.md](adding-an-os.md) - blueprints and bootstrap steps
- [troubleshooting.md](troubleshooting.md)

## Red Hat subscription

Only `redhat-rhel-workstation` reads any of this. Rocky, Fedora and Ubuntu ignore
it entirely - Rocky in particular is the same software as RHEL with public
repositories, and needs no account.

| Setting | Quick name | Meaning |
| --- | --- | --- |
| `redHat.activationKey` | `rhsmActivationKey` | the key itself. **Local file only** |
| `redHat.activationKeyFile` | | a path on this laptop holding just the key |
| `redHat.organizationId` | `rhsmOrgId` | the organization the key belongs to |
| `redHat.username` | `rhsmUsername` | alternative to a key |
| `redHat.password` | `rhsmPassword` | **Local file only** |
| `redHat.autoAttach` | | attach a subscription after registering. Default `true` |
| `redHat.pinRelease` | | pin dnf to the minor release in `osVersion`. Default `true` |

Get the key and the organization id from console.redhat.com, under Connector then
Activation Keys. A no-cost Red Hat Developer subscription is enough.

**Prefer an activation key to a username and password.** A key carries no
password, can be revoked on its own, and is the credential Red Hat builds for
provisioning. Either way the secret is staged as a `0600` file rather than an
environment variable, so it stays out of `/etc/vm-bootstrap.env`, which is
world-readable, and `00-subscription.sh` shreds it once it has registered.

**Without a subscription the build still succeeds**, and that is the trap worth
knowing about. The VM installs and boots with everything on the DVD, but dnf has
no repositories, so bootstrap can add nothing: no EPEL, so no `xrdp`, so no
Enhanced Session Mode. The plan warns at render time and bootstrap warns again,
rather than letting it look like a failure later. Fix it after the fact with:

```bash
sudo subscription-manager register --org <id> --activationkey <key> --auto-attach
sudo vm-bootstrap --only 00-subscription --force
```

**`pinRelease` exists because minor releases are the point.** Someone who asks
for `9.7` usually needs 9.7, because it matches a server they have to reproduce.
Without the pin, the full upgrade in `01-base.sh` would walk the machine onto
whatever minor Red Hat has published since. Clear it inside the guest with
`sudo subscription-manager release --unset`.
