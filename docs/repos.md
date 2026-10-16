# Cloning repositories

Optional. `repos` is empty by default and nothing is cloned. Anything
listed there is cloned into the primary user's home on first boot by
[70-repos.sh](../blueprints/bootstrap/70-repos.sh).

## The short version

```yaml
reposRoot: src
repos:
  - https://github.com/acme/web.git
  - git@github.com:acme/private-api.git
```

That clones into `~/src/web` and `~/src/private-api`. `reposRoot` (long-hand
`repos.root`) is relative to the user's home unless it starts with `/`.

Put this in `.vmynt/vm.config.yaml` for repos the whole team needs, or in
`.vmynt/vm.config.local.yaml` for your own. Because it merges like everything else, a
single machine in a fleet can have its own list:

```yaml
machines:
  ubuntu-middleware-db:
    repos: [git@github.com:acme/db-migrations.git]
```

## Per-repo options

An entry is either a bare URL or a block:

```yaml
repos:
  - url:       https://github.com/acme/tools.git
    directory: team/tools          # defaults to the repo name; may nest
    branch:    develop             # defaults to the default branch
    depth:     1                   # >0 for a shallow clone
    recurse:   true                # --recurse-submodules
    postClone: npm ci && npm run build
```

`postClone` runs inside the clone as the developer, through a login shell, so
`nvm`-managed `node`, `pipx` tools, and `ddev` are all on `PATH`. It is
deliberately explicit - nothing is auto-detected or run behind your back.

Everything is validated at build time, so a typo fails during `vmynt config --validate`
or `vmynt create --render` rather than twenty minutes into a first boot: URLs must be a scheme git accepts,
directories must be relative and free of `..`, and two entries cannot target the
same directory.

## Private repositories

`70-repos.sh` runs after `55-github.sh`, so the SSH key, `known_hosts`, and the
`gh` credential helper are already in place. Which of the two works depends on
your remote:

**`https://` remotes** work as soon as `gh` is authenticated, because
`gh auth setup-git` installs a credential helper. Supply a token:

```yaml
github:
  cli:
    install:   true
    tokenFile: ~/.secrets/gh-token
```

**`git@github.com:` remotes** need the guest's public key registered on the
GitHub account. With `github.sshKey.mode: generate` the key is created *inside* the
guest, so on a first boot it is not on GitHub yet and the clone will fail. Three
ways round that, in order of preference:

1. Let the build register it - a token with the `admin:public_key` scope plus
   `github.uploadSshKey: true`. Fully unattended.
2. Use `https://` remotes instead, with a token.
3. Use `github.sshKey.mode: copy` with a key already on your account - see
   [github-access.md](github-access.md) for that trade-off.

The build warns when it can see this coming:

```
warning: 1 repo(s) use an ssh remote and the key is generated inside the guest, so it is not on
GitHub yet; private clones will fail on the first boot. Supply githubTokenFile with uploadSshKey,
or use https:// remotes.
```

If a clone does fail, nothing is lost. Add the key, then:

```bash
sudo vm-bootstrap --only 70-repos --force
```

## Re-running is safe

A repo that is already cloned is **fetched, never reset or pulled**:

```bash
git -C ~/src/web fetch --all --prune --tags
```

Uncommitted work is never at risk, and no merge happens behind your back. A path
that exists but is not a git repo is skipped with a warning rather than
overwritten.

If any repo fails, the step exits non-zero. That is deliberate: the runner leaves
it un-stamped, so the next `sudo vm-bootstrap` (or the next boot) retries only
the repos, while everything that did clone stays cloned. It also means
`/var/lib/vm-bootstrap/complete` is not written - the machine genuinely is not
finished if a repo you asked for is missing.

## Not the same as dotfiles

`bootstrap.dotfilesRepo` is separate and runs earlier (step `60`). It clones one
repo to `~/.dotfiles` and runs its installer if it has one. Use `repos` for
project code and `dotfilesRepo` for shell configuration.

## Troubleshooting

```bash
sudo vm-bootstrap --only 70-repos --force    # retry
cat /etc/vm-bootstrap.repos                  # what the guest was told to clone
sudo -iu $USER ssh -T git@github.com         # is the key accepted
gh auth status                               # is https auth working
```

**`Permission denied (publickey)`** - the key is not on the account. See above.

**A clone hangs** - it should not: `GIT_TERMINAL_PROMPT=0` turns a missing
credential into an immediate failure rather than a username prompt that would
block bootstrap forever. If it does hang, the remote is unreachable; check DNS
and the proxy.

**`postClone` failed** - the clone succeeded and is left in place; only the
command failed. Run it by hand in the checkout to see the real error.
