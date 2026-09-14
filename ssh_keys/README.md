# ssh_keys

Drop key files in here and the build picks them up. No config editing.

Nothing you put in this directory is committed - see the `.gitignore` beside
this file, which ignores everything except itself and this README.

## What to drop

| File | What happens |
| --- | --- |
| a private key (`id_ed25519`, `github_key`, ...) | installed as the guest's SSH identity, used for `git@github.com:` clones |
| any `.pub` file | added to the guest's `authorized_keys`, so you can `ssh` in from this laptop with no password. Paired or standalone, it makes no difference |
| its `.pub` pair | additionally used as the identity's public half; derived with `ssh-keygen -y` if missing |
| `authorized_keys` | every line added to the guest's `authorized_keys` |

Typical setup - copy in the key you already use for GitHub, plus your laptop's
public key so SSH from the host just works:

```powershell
Copy-Item $HOME\.ssh\id_ed25519     .\ssh_keys\
Copy-Item $HOME\.ssh\id_ed25519.pub .\ssh_keys\
```

Then build. `GitHub.SshKey.Mode` defaults to `auto`, which means: use a private
key from here if there is one, otherwise generate a fresh one inside the guest.

Check what the build made of it before committing to a 30-minute install:

```powershell
.\scripts\New-DevVM.ps1 -RenderOnly
```

It prints the key it chose, for example
`github  ssh key: ssh_keys/id_ed25519, gh cli`.

## Notes

- **One private key** is the normal case. With several, set
  `GitHub.SshKey.Name = 'github_key'` to say which one is the GitHub identity, or
  name it `id_ed25519` / `id_rsa` / `id_ecdsa` and it wins by convention. The
  build refuses to guess.
- **Passphrase-protected keys** are installed and work with git, but they prompt
  for the passphrase - so an unattended repo clone during first boot will fail,
  and the key cannot be uploaded to GitHub unless you drop its `.pub` in too. The
  build warns about both.
- **PuTTY `.ppk` files are not supported.** Convert first:
  PuTTYgen, then Conversions, then Export OpenSSH key.
- **Any other file** is ignored with a warning, so a stray note in here is
  harmless.
- A private key dropped here is written to the VM's seed disk in plain text on
  the way in. The build says so, and bootstrap shreds the staged copy. If that
  matters, leave this directory empty and let the guest generate its own key -
  see [../docs/github-access.md](../docs/github-access.md).
