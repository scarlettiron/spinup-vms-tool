# GitHub access

Everything here is optional - a VM builds and works with all of it turned off.
Configured in the `git` and `github` blocks of `.vmynt/vm.config.yaml` (or the
`gitName`, `gitEmail` and `githubUsername` quick settings), and applied by
bootstrap steps [52-git.sh](../blueprints/bootstrap/52-git.sh) and
[55-github.sh](../blueprints/bootstrap/55-github.sh).

## The drop directory

Copy your key files into the project's `.vmynt/ssh_keys/` and there is nothing to
configure:

```sh
cp ~/.ssh/id_ed25519 ~/.ssh/id_ed25519.pub .vmynt/ssh_keys/
```

or on Windows:

```powershell
Copy-Item $HOME\.ssh\id_ed25519, $HOME\.ssh\id_ed25519.pub .\.vmynt\ssh_keys\
```

Nothing in that folder except its README is committed; `vmynt init` writes a
`.gitignore` that makes sure of it.

`sshKey.mode` defaults to `auto`: a private key in there becomes the guest's identity,
and if the directory has none the guest generates its own. A standalone `.pub`
(no private half) goes to the guest's `authorized_keys` instead, so SSH from this
laptop needs no password.

| In `ssh_keys/` | Result |
| --- | --- |
| nothing | key generated inside the guest |
| one private key | that key becomes the guest's identity |
| a private key + its `.pub` | same, and the `.pub` is used as-is |
| a private key with no `.pub` | `.pub` derived with `ssh-keygen -y` |
| a standalone `.pub` | added to the guest's `authorized_keys` |
| several private keys | set `github.sshKey.name`, or name one `id_ed25519` |

Vmynt also adds its own public key (`~/.vmynt/ssh/id_ed25519.pub`) to every
guest's `authorized_keys`, so `vmynt ssh`, `exec`, `logs` and `status` work
without any of this.

## Your GitHub username

Optional, and nothing depends on it: `gh` gets the account from the token, and an
SSH key does not need a username at all. It earns its place in two smaller ways.

```yaml
githubUsername: adalovelace     # or github.username long-hand
```

- **`git config --global github.user`** in the guest, which `hub` and some editor
  integrations read, and which makes `git config --list` say which account a VM is
  pointed at.
- **A verification check.** After the key is installed, bootstrap runs
  `ssh -T git@github.com` and compares the greeting to the configured name:

  ```
  [12:04:31] github ssh auth ok as 'adalovelace'
  ```

  If they differ you get a warning, which is the useful case: it means the key in
  `.vmynt/ssh_keys/` belongs to a different account than you thought.

  ```
  ! GitHub greeted 'olduser' but GitHub.Username says 'adalovelace'
  !   the key belongs to a different account than you configured
  ```

Leave it empty and the check still runs and still reports which account
authenticated, it just has nothing to compare against. GitHub always exits
non-zero for `ssh -T` because the account has no shell, so the greeting text is
the only signal, and being unable to reach github.com during first boot is logged
rather than treated as a failure.

## Keeping the private key off the seed disk

Leave `.vmynt/ssh_keys/` empty (or set `mode: generate`) and put your identity in
config instead:

```yaml
gitName:  Ada Lovelace
gitEmail: ada@example.com
github:
  sshKey:
    mode: generate
  cli:
    install: true
```

The guest generates its own ed25519 keypair on first boot. **No private key is
ever written to the seed disk, and none has to exist on your laptop.** The public
half shows up in three places:

- the login banner (`/etc/motd`) and `/etc/vm-bootstrap.summary`
- the bootstrap log, `/var/log/vm-bootstrap.log`
- `cat ~/.ssh/id_ed25519.pub` in the guest

Paste it into <https://github.com/settings/keys> once and the VM can clone.

## Letting the build register the key for you

Supply a personal access token with the `admin:public_key` scope and the key is
uploaded automatically, so a fresh VM can clone private repos with no manual
step at all:

```yaml
github:
  sshKey:
    mode: generate
  cli:
    install:   true
    tokenFile: ~/.secrets/gh-token
  uploadSshKey: true
```

`tokenFile` reads the token from a path on your laptop **at build time**, which
keeps it out of the repo entirely. `~/` and `%VAR%`/`$VAR` work in it, and a
relative path is relative to the project folder. `cli.token` takes one inline
instead - only put that in `vm.config.local.yaml`, which is git-ignored.

The token is also used for `gh auth login`, so `gh` is signed in when you first
open a terminal. `gh auth setup-git` runs too, so HTTPS remotes work.

## Bringing your own key

Drop it into `.vmynt/ssh_keys/` as above, or point at a path if you would rather
not copy it into the project at all:

```yaml
github:
  sshKey:
    mode:           copy
    privateKeyPath: ~/.ssh/id_ed25519
    # publicKeyPath defaults to <privateKeyPath>.pub
```

`mode: copy` differs from `auto` only in failing loudly when no key can be
found, rather than quietly generating one. Either route is the right choice if
you want one key across several VMs. Be aware of the trade-off, which the build
warns about:

```
warning: embedding a secret in the seed disk: private SSH key from /home/ada/.ssh/id_ed25519
```

The key travels on the seed disk in plain text. Step 55 installs it to
`~/.ssh/`, then `shred`s the staged copy (`github.shredSecretsAfterUse`).
Vmynt detaches the seed disk and deletes its file once the install has finished,
so it does not linger on the host. What remains is the installer's own record of
the payload in the guest, such as `/var/lib/cloud/instance/user-data.txt` on
Ubuntu (root-readable only), and the review copy `vmynt create` keeps in the VM's
`build/` folder on the host (private permissions).

If any of that is uncomfortable, use `mode: generate` - it has none of these
properties because there is no secret to move.

## Passphrases

Generated keys have no passphrase, deliberately: the key exists so an unattended
local dev VM can reach GitHub, and a passphrase would mean a prompt in the middle
of provisioning.

A passphrase-protected key you supply still works - it is installed and git uses
it normally - but the build cannot derive its public half, so it cannot be
uploaded to GitHub, and an unattended repo clone during first boot will fail
waiting for the passphrase. Drop the `.pub` file in alongside it to at least fix
the first of those. The build warns in both cases.

## What else the steps do

- **`~/.ssh/config`** gets a `Host github.com` stanza pointing at the key with
  `IdentitiesOnly yes`, so ssh offers the right key even when the agent holds
  several.
- **`known_hosts`** is seeded with `ssh-keyscan github.com`, so the first clone
  does not stop to ask you to confirm a host key. Set `github.knownHosts: false`
  to skip it.
- **Git config** from the `git` block, including arbitrary pairs:
  ```yaml
  git:
    extra:
      push.autoSetupRemote: 'true'
      rerere.enabled: 'true'
  ```

## Verifying, in the guest

```bash
ssh -T git@github.com          # expect: Hi <you>! You've successfully authenticated
gh auth status
git config --global --list
sudo vm-bootstrap --only 55-github --force   # re-run just this step
```

## Troubleshooting

**`Permission denied (publickey)`** - the key was generated but never added to
GitHub. `cat ~/.ssh/id_ed25519.pub` and add it, or supply a token with
`admin:public_key` and re-run the step.

**`could not upload the key`** - the token lacks the `admin:public_key` scope, or
the key is already registered on the account.

**`gh auth login failed`** - the token is expired or malformed. The build
rejects a token containing whitespace, so a stray newline is not the cause; check
it at <https://github.com/settings/tokens>.

**The key is missing after a rebuild from a checkpoint taken before bootstrap** -
expected. Re-run `sudo vm-bootstrap --only 55-github`.
