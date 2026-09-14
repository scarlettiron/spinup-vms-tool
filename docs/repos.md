# Cloning repositories

Optional. `Repos.Items` is empty by default and nothing is cloned. Anything
listed there is cloned into the primary user's home on first boot by
[70-repos.sh](../blueprints/bootstrap/70-repos.sh).

## The short version

```powershell
Repos = @{
    Root  = 'src'
    Items = @(
        'https://github.com/acme/web.git'
        'git@github.com:acme/private-api.git'
    )
}
```

That clones into `~/src/web` and `~/src/private-api`. `Root` is relative to the
user's home unless it starts with `/`.

Put this in `vm.config.yaml` for repos the whole team needs, or in
`vm.config.local.yaml` for your own. Because it merges like everything else, a
single machine in a fleet can have its own list:

```powershell
Machines = @{
    'ubuntu-middleware-db' = @{
        Repos = @{ Items = @('git@github.com:acme/db-migrations.git') }
    }
}
```

## Per-repo options

An entry is either a bare URL string or a hashtable:

```powershell
Items = @(
    @{
        Url       = 'https://github.com/acme/tools.git'
        Directory = 'team/tools'          # defaults to the repo name; may nest
        Branch    = 'develop'             # defaults to the default branch
        Depth     = 1                     # >0 for a shallow clone
        Recurse   = $true                 # --recurse-submodules
        PostClone = 'npm ci && npm run build'
    }
)
```

`PostClone` runs inside the clone as the developer, through a login shell, so
`nvm`-managed `node`, `pipx` tools, and `ddev` are all on `PATH`. It is
deliberately explicit - nothing is auto-detected or run behind your back.

Everything is validated at build time, so a typo fails during `-RenderOnly`
rather than twenty minutes into a first boot: URLs must be a scheme git accepts,
directories must be relative and free of `..`, and two entries cannot target the
same directory.

## Private repositories

`70-repos.sh` runs after `55-github.sh`, so the SSH key, `known_hosts`, and the
`gh` credential helper are already in place. Which of the two works depends on
your remote:

**`https://` remotes** work as soon as `gh` is authenticated, because
`gh auth setup-git` installs a credential helper. Supply a token:

```powershell
GitHub = @{ Cli = @{ Install = $true; TokenFile = 'C:\Users\you\.secrets\gh-token' } }
```

**`git@github.com:` remotes** need the guest's public key registered on the
GitHub account. With `SshKey.Mode = 'generate'` the key is created *inside* the
guest, so on a first boot it is not on GitHub yet and the clone will fail. Three
ways round that, in order of preference:

1. Let the build register it - a token with the `admin:public_key` scope plus
   `UploadSshKey = $true`. Fully unattended.
2. Use `https://` remotes instead, with a token.
3. Use `SshKey.Mode = 'copy'` with a key already on your account - see
   [github-access.md](github-access.md) for that trade-off.

The build warns when it can see this coming:

```
! 1 repo(s) use an ssh remote and the key is generated inside the guest, so it
  is not on GitHub yet - private clones will fail on the first boot.
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

`Bootstrap.DotfilesRepo` is separate and runs earlier (step `60`). It clones one
repo to `~/.dotfiles` and runs its installer if it has one. Use `Repos` for
project code and `DotfilesRepo` for shell configuration.

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

**`PostClone` failed** - the clone succeeded and is left in place; only the
command failed. Run it by hand in the checkout to see the real error.
