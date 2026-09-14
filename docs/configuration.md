# Configuration

Two layers of YAML. The team standard is committed; your own overrides are not.

| Layer | File | Committed? |
| --- | --- | --- |
| team standard | `vm.config.yaml` | yes |
| per-developer | `vm.config.local.yaml` | no, git-ignored |

Copy the example to start:

```powershell
Copy-Item vm.config.local.example.yaml vm.config.local.yaml
```

Your local file only names what you want to differ. Everything else falls through
to `vm.config.yaml`.

**One file per layer.** The loader also accepts `.psd1` for a layer, because
PowerShell data files need no parser and the blueprints use that format already.
Nothing ships in it, and if both formats exist for the same layer the build stops
and says so rather than guessing which you meant.

## Reading the config file

Every section carries a tag saying whether it is meant to be edited:

| Tag | Means |
| --- | --- |
| `@user-editable` | change these freely; that is what they are there for |
| `@advanced` | the default is right for almost everyone, and the comment next to it says what changing it costs |

Grep for the tag when you are looking for somewhere to change something:

```powershell
Select-String '@user-editable' vm.config.yaml
```

`@user-editable` covers: the quick settings block, `VM.DataDisks`,
`Guest.PrimaryUser`, `Guest.AdditionalUsers`, `Bootstrap`, `Git`, `GitHub`
(`SshKey`, `Cli`, `UploadSshKey`), and `Machines`.

`@advanced` covers the rest of `VM`, `Guest.Ssh`, `GitHub.KnownHosts`,
`GitHub.ShredSecretsAfterUse`, and `Iso`. Nothing there is off-limits, it just
has a default that is right unless you have a specific reason.

Files under `blueprints/` are a level below all of this: they describe what an OS
*is* rather than how your VM is configured. See
[adding-an-os.md](adding-an-os.md).

## Quick settings

The handful of settings people actually change are available as flat names, so
they can sit in one block at the top of the file instead of nested four levels
deep. Each is a shortcut for exactly one real setting:

| Quick setting | Is really | |
| --- | --- | --- |
| `ProjectName` | (top level) | names the project; VM and hostname default to `<ProjectName>-<Blueprint>` |
| `VmName` | `VM.Name` | overrides the derived VM name |
| `VmPath` | `VM.Path` | where the VM and its disks live |
| `Cpus` | `VM.CpuCount` | |
| `MemoryGB` | `VM.MemoryStartupGB` | |
| `MaxMemoryGB` | `VM.MemoryMaximumGB` | dynamic-memory ceiling |
| `DiskGB` | `VM.DiskSizeGB` | the **system** disk; others are `VM.DataDisks` |
| `Switch` | `VM.SwitchName` | |
| `EnhancedSession` | `VM.EnhancedSession` | clipboard + resizable desktop |
| `Hostname` | `Guest.Hostname` | overrides the derived hostname; also `<hostname>.local` |
| `Username` | `Guest.PrimaryUser.Username` | |
| `RealName` | `Guest.PrimaryUser.RealName` | |
| `Password` | `Guest.PrimaryUser.Password` | hashed at build time |
| `PasswordHash` | `Guest.PrimaryUser.PasswordHash` | supply your own crypt hash |
| `Timezone` | `Guest.Timezone` | |
| `Locale` | `Guest.Locale` | |
| `KeyboardLayout` | `Guest.KeyboardLayout` | |
| `GitName` | `Git.UserName` | |
| `GitEmail` | `Git.UserEmail` | |
| `GithubUsername` | `GitHub.Username` | your GitHub account name |
| `GithubToken` | `GitHub.Cli.Token` | put this in the local file only |
| `GithubTokenFile` | `GitHub.Cli.TokenFile` | a path on this laptop |
| `SshKeyMode` | `GitHub.SshKey.Mode` | `auto` / `generate` / `copy` / `none` |
| `Repos` | `Repos.Items` | when given a list |
| `ReposRoot` | `Repos.Root` | |
| `OsVersion` | `Iso.Release` | the OS release; blank uses the blueprint's `Os.Version` |
| `Release` | `Iso.Release` | synonym for `OsVersion` |
| `UbuntuRelease` | `Iso.Release` | synonym, kept for older configs |
| `IsoPath` | `Iso.Path` | use a local .iso |
| `InstallDesktop` | `Bootstrap.InstallDesktop` | |
| `InstallDocker` | `Bootstrap.InstallDocker` | |
| `InstallDdev` | `Bootstrap.InstallDdev` | |
| `InstallNode` | `Bootstrap.InstallNode` | |
| `InstallPython` | `Bootstrap.InstallPython` | |
| `InstallVSCode` | `Bootstrap.InstallVSCode` | |
| `InstallChrome` | `Bootstrap.InstallChrome` | Google Chrome; skipped without a desktop |
| `InstallRemoteAccess` | `Bootstrap.InstallRemoteAccess` | VPN, RDP and SSH tunnelling clients; GUI parts need a desktop |
| `NodeVersion` | `Bootstrap.NodeVersion` | any nvm alias |
| `Resolution` | `Bootstrap.DisplayResolution` | |
| `ExtraPackages` | `Bootstrap.ExtraAptPackages` | |
| `DotfilesRepo` | `Bootstrap.DotfilesRepo` | |
| `EnableMdns` | `Bootstrap.EnableMdns` | |
| `RebootWhenDone` | `Bootstrap.RebootWhenDone` | |
| `DbEngine` | `Database.Engine` | `postgres` / `mysql`; blank installs neither |
| `DbVersion` | `Database.Version` | blank takes the engine default: postgres 17, mysql 8 |
| `DbClient` | `Database.InstallClient` | the command line client only |
| `DbServer` | `Database.InstallServer` | the server too; implies the client |
| `DbPort` | `Database.Port` | 0 takes the engine default: 5432 / 3306 |
| `DbListenAddress` | `Database.ListenAddress` | `localhost` or `*` |
| `DbAdminPassword` | `Database.AdminPassword` | put this in the local file only |
| `DbDatabases` | `Database.Databases` | names to create on first boot |
| `DbAdminer` | `Database.Adminer.Install` | the single-file database browser; needs `DbEngine` |
| `DbAdminerPort` | `Database.Adminer.Port` | where Adminer is served, default 3800 |
| `CopyFrom` | `Files.Source` | a local directory whose contents are copied into the guest |
| `CopyTo` | `Files.Destination` | where they land; blank means the primary user's home |
| `PostStart` | `Hooks.PostStart` | commands to run once the VM is provisioned |
| `PostStartFile` | `Hooks.PostStartFile` | a local script, appended after those commands |
| `PostStartRunAs` | `Hooks.RunAs` | `user` (default) or `root` |
| `PostStartEveryBoot` | `Hooks.RunAtEveryBoot` | also run on every subsequent boot |

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

Shortcuts work inside a `Machines` block too.

**Names are case-insensitive.** `cpus`, `Cpus`, and `CPUS` are the same setting,
so YAML can stay lower-case and `.psd1` can stay PascalCase.

**Typos are rejected.** An unrecognised top-level name fails the build with the
list of valid ones, rather than being silently ignored:

```
BUILD FAILED: Unknown setting 'Cpuz' in vm.config.local.yaml.
Valid top-level settings are: Bootstrap, Cpus, DiskGB, ...
```

## Naming, and the address you browse to

`ProjectName` names the work rather than the OS. The VM and the guest hostname
both default to `<ProjectName>-<Blueprint>`:

```powershell
ProjectName = 'acme'
Blueprint   = 'fedora-workstation'
#  VM name and hostname become: acme-fedora-workstation
```

`Bootstrap.EnableMdns` publishes that hostname with avahi, so it is also the
address a browser uses, with no hosts file and no static IP:

```
http://acme-fedora-workstation.local
```

Change `ProjectName` and every machine in the project is renamed together.

**Precedence, highest first:** an explicit `VmName` / `Hostname`, then a
`Machines` key (a machine is named after its key), then
`<ProjectName>-<Blueprint>`.

The derived name is lower-cased, and both it and an explicit `Hostname` are
checked as DNS labels, because a name that is not one will not resolve over mDNS
and the address would be useless:

- letters, digits and hyphens only, not starting or ending with a hyphen
- 63 characters maximum

`ProjectName` is checked the same way, and rejected if it would produce an
invalid label. All of these fail at render time, not after a 30-minute build.

### The name-is-taken check

A real build refuses to start if the name is already in use, which catches two
different collisions:

| Clash | Detected by | Result |
| --- | --- | --- |
| a Hyper-V VM of that name exists | `Get-VM` | fails; `-Force` replaces it |
| `<hostname>.local` already answers | the OS resolver, which does mDNS | fails |

The second matters because two machines claiming one mDNS name means avahi
renames the loser to `<hostname>-2.local`, so the address you hand a colleague
quietly reaches the wrong machine. It is not a Hyper-V question: the name might
belong to a teammate's laptop on the same switch.

`-Force` skips the hostname check when it is replacing a VM of that name, since
the thing answering is almost certainly the VM being replaced.

`-RenderOnly` reports a taken hostname as a warning rather than a failure, since
nothing is being created. `-All` additionally refuses two machines that resolve to
the same VM name or the same hostname.

## Choosing the OS and its version

`Blueprint` and `OsVersion` sit together at the top of the config because they are
coupled: `43` is a Fedora release and means nothing to Ubuntu, `24.04` is an
Ubuntu release and means nothing to Fedora.

| Blueprint | `OsVersion` looks like | Support |
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
BUILD FAILED: OsVersion '43' does not look like a ubuntu-desktop release.
Its own version is 24.04. Set OsVersion to match the blueprint, or blank it
to use the blueprint's own.
```

**Two ways to run this, and the trade-off is real:**

- **Name the version explicitly** (what ships). Clear at a glance, but every
  `Machines` entry on a different blueprint has to name its own version too,
  which is why both Ubuntu machines carry `OsVersion = '24.04'`.
- **Leave `OsVersion` blank.** Each blueprint then supplies its own
  (`Os.Version`), so a mixed fleet needs no version keys at all and switching OS
  is genuinely one line. Less explicit, fewer places to keep in step.

To pin a release for the long term, set it in the blueprint's `Os.Version`, not in
config: that way it travels with the OS it belongs to.

## Precedence

Lowest to highest. Set a key in whichever layer it belongs to; the rest is
inherited:

| | Layer | Purpose |
| --- | --- | --- |
| 1 | `blueprint.Defaults` | what the OS *suggests* - sizing, nice-to-haves |
| 2 | `vm.config.*` | the team standard |
| 3 | `blueprint.Overrides` | what the OS *is* - a server blueprint has no desktop |
| 4 | `vm.config.local.*` | per-developer |
| 5 | `Machines.<name>` | the per-machine block, with `-Machine` |
| 6 | `-Parameter` | one-off, on the command line |

Layers 1 and 3 both come from the selected blueprint and deliberately sit on
opposite sides of the team standard - see
[adding-an-os.md](adding-an-os.md#inheriting).

Quick settings are expanded **per layer, before merging**, so precedence reads
the way you would expect: `cpus: 8` in your local file beats
`VM: { CpuCount: 6 }` in the shared file, and vice versa.

Nested settings merge key by key. Lists replace wholesale, so an override can
shorten a package list rather than only ever growing it.

## Seeing the result

```powershell
.\scripts\New-DevVM.ps1 -RenderOnly
```

Prints the resolved plan and writes the fully rendered cloud-init plus every
generated bootstrap file to `build\<vm-name>\`. No elevation, no Hyper-V
changes - the fastest way to check a config edit did what you meant.

## The YAML reader

Windows PowerShell has no YAML support, and requiring `Install-Module
powershell-yaml` before anyone can build a VM would defeat the point of a repo
you clone and run. So `scripts/lib/Common.psm1` carries a small self-contained
reader for the subset of YAML that config files need. It is verified against
PyYAML.

**Supported:**

```yaml
key: value                  # mappings, nested by indentation
list:
  - one                     # sequences, at or below the key's indent
  - two
list2:
- also fine
records:
  - name: a                 # sequences of mappings
    size: 1
flow: [a, b, c]             # flow sequences
empties: []                 # and {}
text:   plain, 'single', "double with \n and \t"
numbers: 42, -3, 1.5
bools:  true false yes no on off      # any case
nulls:  null, ~, or nothing at all
# comments, honouring quotes
---                         # a single leading document marker
```

**Not supported** - rejected with a clear message rather than misread: anchors
and aliases (`&x` / `*x`), tags (`!!str`), block scalars (`|` and `>`), multiple
documents in one file, and flow mappings other than `{}`. None of them are needed
to express a config, and every one of them is a way to write something the next
person cannot read. A nested mapping goes on its own indented lines.

**Two things worth knowing:**

- `yes`, `no`, `on`, `off` are booleans, as in YAML 1.1 and PyYAML. Quote them
  (`'no'`) when you mean the string.
- A number with a leading zero stays a **string**: `007` is far more likely to be
  an identifier than octal. PyYAML would read it as a number, so this is the one
  place the reader deliberately differs.

Values that look alarming are fine unquoted, because a colon only separates a key
when followed by whitespace: `C:\VMs`, `git@github.com:acme/web.git`, `lts/*`,
`en_US.UTF-8`, `1920x1080`. Quote a release number though - `24.04` unquoted is
the number 24.04, not the string.

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
| `RedHat.ActivationKey` | `RhsmActivationKey` | the key itself. **Local file only** |
| `RedHat.ActivationKeyFile` | | a path on this laptop holding just the key |
| `RedHat.OrganizationId` | `RhsmOrgId` | the organization the key belongs to |
| `RedHat.Username` | `RhsmUsername` | alternative to a key |
| `RedHat.Password` | `RhsmPassword` | **Local file only** |
| `RedHat.AutoAttach` | | attach a subscription after registering. Default `true` |
| `RedHat.PinRelease` | | pin dnf to the minor release in `OsVersion`. Default `true` |

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

**`PinRelease` exists because minor releases are the point.** Someone who asks
for `9.7` usually needs 9.7, because it matches a server they have to reproduce.
Without the pin, the full upgrade in `01-base.sh` would walk the machine onto
whatever minor Red Hat has published since. Clear it inside the guest with
`sudo subscription-manager release --unset`.
