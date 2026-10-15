# Releasing

How Vmynt gets from a git tag to `winget install`, `choco install`, `brew install`
and `apt install`, what has to be set up once before that works, and what to do when
it doesn't.

The short version, once everything is set up:

1. Every pull request adds a line under `## [Unreleased]` in `CHANGELOG.md`.
2. **Actions → Version bump → Run workflow**, pick `patch`, `minor` or `major`.

That bumps `Cargo.toml`, moves the changelog notes under the new version, commits,
tags `vX.Y.Z`, and starts the **Release** workflow (`.github/workflows/release.yml`),
which builds and publishes everything. Details in
[Versioning and releasing](#versioning-and-releasing).

---

## How it fits together

| Channel | Users run | What the workflow does | Secret | Visible to users after |
|---|---|---|---|---|
| GitHub Release | download from the releases page | builds 6 archives, 2 `.deb`, 2 `.rpm`, writes `SHA256SUMS` | none (uses `GITHUB_TOKEN`) | the run finishes |
| Homebrew (macOS, Linux) | `brew install scarlettiron/tap/vmynt` | writes `Formula/vmynt.rb` in the tap repo | `HOMEBREW_TAP_TOKEN` | the push (minutes) |
| winget (Windows) | `winget install CodeScarlett.Vmynt` | opens a PR on `microsoft/winget-pkgs` | `WINGET_TOKEN` | Microsoft merges the PR (hours to a few days) |
| Chocolatey (Windows) | `choco install vmynt` | packs, test-installs and pushes the `.nupkg` | `CHOCOLATEY_API_KEY` | moderation passes (first version: days; later: often minutes) |
| apt (Debian, Ubuntu) | see below | re-runs the Pages workflow, which rebuilds the signed repo at `/apt/` | `APT_GPG_PRIVATE_KEY` (+ `APT_GPG_PASSPHRASE`) | Pages deploys (minutes) |

apt install, as published on the site:

```bash
curl -fsSL https://scarlettiron.github.io/vmynt/apt/vmynt-archive-keyring.gpg | sudo tee /usr/share/keyrings/vmynt-archive-keyring.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/vmynt-archive-keyring.gpg] https://scarlettiron.github.io/vmynt/apt stable main" | sudo tee /etc/apt/sources.list.d/vmynt.list
sudo apt update && sudo apt install vmynt
```

Every package manager job checks for its secret first. A missing secret skips that
channel with a notice on the run summary; it never fails the release. So you can set
the channels up one at a time.

### Where the names come from

Nothing in the workflows or packaging scripts types the product name. They all read
`Cargo.toml` through `packaging/brand.py`:

| `Cargo.toml` | Used for |
|---|---|
| `[package] name` | binary, archive names, formula file, apt package, keyring file |
| `[package] version` | must equal the tag without its `v` |
| `[package] description` | package descriptions (the part before " — " is the short one) |
| `[package] repository` | download urls (`owner/repo` is taken from it) |
| `[package] homepage` | homepage fields |
| `[package.metadata.brand] display-name` | titles, release name, apt repo label |
| `[package.metadata.brand] winget-id` / `winget-publisher` | winget |
| `[package.metadata.brand] chocolatey-id` | Chocolatey |
| `[package.metadata.brand] homebrew-tap` | which repo the formula is pushed to |
| `[package.metadata.brand] site-url` / `apt-url` | links, and the apt instructions page |

`python3 packaging/brand.py` prints every value. `python3 packaging/render.py --list`
prints every template placeholder.

### Files

```
.github/workflows/ci.yml           fmt, clippy, build and tests on Linux/Windows/macOS, the minimum
                                   Rust version, packaging/test.sh, version.py, CHANGELOG.md,
                                   actionlint, shellcheck and the website's links
.github/workflows/version-bump.yml the "cut a release" button: bump, changelog, commit, tag
.github/workflows/release.yml      the release (on a v* tag, or started by version-bump.yml)
.github/workflows/pages.yml        site/ + the apt repository to GitHub Pages
.github/workflows/e2e.yml          nightly: builds a real VM under libvirt/KVM, end to end
.github/dependabot.yml             weekly grouped updates for crates and actions
.github/actionlint.yaml            actionlint settings (the windows-11-arm runner label)
packaging/version.py               reads and bumps the version, edits CHANGELOG.md
packaging/check_site.py            link, anchor and SEO-tag check for site/
packaging/brand.py                 reads names and ids from Cargo.toml
packaging/render.py             fills the templates below from Cargo.toml + SHA256SUMS
packaging/homebrew/             formula template
packaging/chocolatey/           nuspec + install script templates
packaging/winget/               manifest templates, for the first submission only
packaging/apt/build-repo.sh     builds and signs the apt repository
packaging/apt/make_index.py     writes its Packages and Release files
packaging/test.sh               tests all of the above locally, publishes nothing
```

---

## One-time setup

Do these in order. Each step says which secret it produces. Secrets go in
**Settings → Secrets and variables → Actions → New repository secret**.

### 0. Before the repository goes public

- Make sure no branch has client or employer names, private repository URLs or
  passwords in it, including in personal `vm.config.yaml` files. Anything in git
  history stays public even after a new commit removes it, so either start the public
  repository from a fresh history, or rewrite it (`git filter-repo`) and delete any
  branch that should stay private.
- Scan the whole history for secrets before flipping the switch, for example with
  [gitleaks](https://github.com/gitleaks/gitleaks): `gitleaks git -v`.
- winget and Chocolatey moderators, and Homebrew users, download from public GitHub
  Release urls. Those only work once the repository is public.

### 1. Rename the repository

The urls in `Cargo.toml` already point at `scarlettiron/vmynt`.

1. GitHub → the repository → **Settings → General → Repository name** →
   `vmynt` → Rename. GitHub redirects the old name, but update your clone anyway:
   ```bash
   git remote set-url origin git@github.com:scarlettiron/vmynt.git
   ```
2. Add `Cargo.lock` to git if it isn't already. Every build uses `--locked`.

### 2. GitHub Pages

**Settings → Pages → Build and deployment → Source: GitHub Actions.**

The site then deploys at `https://scarlettiron.github.io/vmynt/` on the next push that
touches `site/`, or from **Actions → Deploy site and apt repository → Run workflow**.

Leave **Settings → Environments → github-pages** at its default (deployments from
`main` only). The release workflow starts the Pages workflow on `main` for exactly
this reason.

### 3. Homebrew tap → `HOMEBREW_TAP_TOKEN`

1. Create a **public** repository named `homebrew-tap` under `scarlettiron`, with a
   README (so it has a `main` branch). The `homebrew-` prefix is what lets users type
   `scarlettiron/tap`.
2. Create a fine-grained personal access token:
   **GitHub → Settings → Developer settings → Personal access tokens → Fine-grained
   tokens → Generate new token**
   - Resource owner: `scarlettiron`
   - Repository access: **Only select repositories → `homebrew-tap`**
   - Permissions → Repository permissions → **Contents: Read and write**
   - Expiration: up to a year. Put a reminder in your calendar; an expired token
     shows up as a 403 in the Homebrew job.
3. Save it as the secret **`HOMEBREW_TAP_TOKEN`**.

The workflow creates `Formula/vmynt.rb` on the first release.

### 4. winget → `WINGET_TOKEN`

1. Fork [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) to
   `scarlettiron` (keep the name `winget-pkgs`).
2. Create a **classic** personal access token (the winget action opens a pull request
   from your fork to Microsoft's repository, which fine-grained tokens can't do):
   **Developer settings → Personal access tokens → Tokens (classic) → Generate new
   token (classic)**, scope **`public_repo`** only.
3. **Don't add the secret yet.** The workflow can only *update* a package that already
   exists in winget-pkgs. Release v0.1.0 (or whatever the first stable version is)
   without it, do the [first winget submission](#first-winget-submission) by hand, and
   add **`WINGET_TOKEN`** once Microsoft has merged that pull request.

### 5. Chocolatey → `CHOCOLATEY_API_KEY`

1. Register at [community.chocolatey.org](https://community.chocolatey.org/account/Register).
   The nuspec lists the repository owner (`scarlettiron`) as the package owner, so use
   that as the Chocolatey username, or change `<owners>` in
   `packaging/chocolatey/package.nuspec.tmpl`.
2. Copy the API key from your account page.
3. Save it as the secret **`CHOCOLATEY_API_KEY`**.

The package id `vmynt` is reserved by the first push.

### 6. apt signing key → `APT_GPG_PRIVATE_KEY`, `APT_GPG_PASSPHRASE`

apt refuses an unsigned repository, so the workflow signs it with a key only it holds.
Make the key on your own machine, in a throwaway keyring so it never mixes with your
personal keys:

```bash
export GNUPGHOME="$(mktemp -d)"

# RSA 4096 is understood by every apt version still in use. No expiry on purpose:
# an expired key breaks "apt update" for every user until they re-download the
# keyring, so rotate it deliberately instead (see "Rotating the apt key").
gpg --batch --pinentry-mode loopback --passphrase 'CHOOSE-A-PASSPHRASE' \
    --quick-gen-key "Vmynt apt repository" rsa4096 sign never

gpg --list-secret-keys --keyid-format long          # note the fingerprint

gpg --pinentry-mode loopback --passphrase 'CHOOSE-A-PASSPHRASE' \
    --armor --export-secret-keys > apt-signing-key.asc
# gpg already wrote a revocation certificate when it made the key; keep a copy.
cp "$GNUPGHOME"/openpgp-revocs.d/*.rev apt-signing-key-revoke.asc
```

1. Save the **contents** of `apt-signing-key.asc` (the whole
   `-----BEGIN PGP PRIVATE KEY BLOCK-----` block) as **`APT_GPG_PRIVATE_KEY`**.
2. Save the passphrase as **`APT_GPG_PASSPHRASE`**. (A key with no passphrase works
   too; then leave this secret out.)
3. Put `apt-signing-key.asc`, the passphrase and `apt-signing-key-revoke.asc` in your
   password manager, then delete the files and the temporary folder:
   ```bash
   rm -rf "$GNUPGHOME" apt-signing-key.asc apt-signing-key-revoke.asc
   unset GNUPGHOME
   ```

The public half is exported by the workflow on every deploy, to
`/apt/vmynt-archive-keyring.gpg` (and `.asc`), and the fingerprint is shown on
`/apt/`.

### Every secret

| Secret | Required for | Notes |
|---|---|---|
| `HOMEBREW_TAP_TOKEN` | Homebrew | fine-grained PAT, Contents read/write on `homebrew-tap` only |
| `WINGET_TOKEN` | winget | classic PAT, `public_repo`; add after the first manual submission |
| `CHOCOLATEY_API_KEY` | Chocolatey | from your community.chocolatey.org account |
| `APT_GPG_PRIVATE_KEY` | apt | ASCII-armoured private key |
| `APT_GPG_PASSPHRASE` | apt | only if the key has a passphrase |
| `RELEASE_TOKEN` | optional, Version bump | fine-grained PAT on this repo only: Contents read/write, Actions read/write. See below |

`GITHUB_TOKEN` is provided automatically; nothing to set up.

**`RELEASE_TOKEN`, and why it is optional.** A tag pushed with the built-in
`GITHUB_TOKEN` does not start other workflows (GitHub's guard against loops). Without
`RELEASE_TOKEN`, the Version bump workflow pushes with `GITHUB_TOKEN` and then starts
the Release workflow on the new tag itself (`gh workflow run release.yml --ref vX.Y.Z`).
With it, the push is made with the token and the tag push starts the Release workflow
the normal way. The result is the same. You need `RELEASE_TOKEN` when `main` has
branch protection that requires pull requests: give the token's owner a bypass in the
ruleset (or allow the GitHub Actions app), otherwise the release commit is refused.

---

## Versioning and releasing

### The rules

- **One source of truth:** `version` in `Cargo.toml`. The binary (`vmynt --version`),
  every package, the archive names and the release notes read it from there.
  `packaging/version.py` is the only thing that writes it.
- **Semantic versioning.** `MAJOR.MINOR.PATCH`, with prereleases as
  `1.2.0-alpha.1` → `-beta.1` → `-rc.1`. Until 1.0, a minor bump may break the CLI or
  config format; say so in the changelog.
- **Tags are `v` + the version**, exactly. The Release workflow refuses any other tag
  (`version.py check-tag`), so a typo can never publish the wrong version.
- **`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).**
  Each pull request adds its line under `## [Unreleased]`, in `### Added`, `### Changed`,
  `### Fixed`, `### Removed`, `### Deprecated` or `### Security`. CI fails if the
  `## [Unreleased]` heading goes missing, or if `Cargo.toml` holds a released version
  with no section of its own.

`packaging/version.py` does the arithmetic, so nobody has to:

```bash
python3 packaging/version.py current                 # 0.1.0
python3 packaging/version.py bump minor              # 0.1.0 -> 0.2.0
python3 packaging/version.py bump minor --pre rc     # 0.1.0 -> 0.2.0-rc.1
python3 packaging/version.py bump prerelease         # 0.2.0-rc.1 -> 0.2.0-rc.2
python3 packaging/version.py bump release            # 0.2.0-rc.2 -> 0.2.0
python3 packaging/version.py changelog 0.2.0         # Unreleased -> ## [0.2.0] - today
python3 packaging/version.py notes 0.2.0             # print that section
python3 packaging/version.py check-tag v0.2.0        # exit 1 on a mismatch
python3 packaging/version.py selftest                # its own tests (CI runs these)
```

### Cutting a release

1. Make sure CI is green on `main`, and `## [Unreleased]` in `CHANGELOG.md` says what
   changed.
2. **Actions → Version bump → Run workflow** on `main`. Choose the bump:
   - `patch` for fixes, `minor` for features, `major` for breaking changes (after 1.0)
   - tick **Dry run** first if you want to see the new version and release notes in
     the run summary without committing anything
3. The workflow commits `release: vX.Y.Z` (Cargo.toml, Cargo.lock, CHANGELOG.md), tags
   it, pushes both in one atomic push, and starts **Actions → Release**.
4. Watch **Release**. In order: *Read Cargo.toml* (fails fast if the tag and version
   disagree), six *Build* jobs, *GitHub Release* (notes from the changelog section,
   then GitHub's list of merged pull requests), then Homebrew, winget, Chocolatey and
   apt in parallel. The apt job re-runs **Pages**, which also republishes the site.
5. Afterwards: check the winget pull request on `microsoft/winget-pkgs` for bot
   comments, and the Chocolatey package page for moderation messages.

A failed package manager job can be re-run on its own from the run page
(**Re-run failed jobs**). The GitHub Release is not recreated. To rerun the whole
release for an existing tag, use **Actions → Release → Run workflow** and type the tag.

**By hand**, without the Version bump workflow (for example offline):

```bash
python3 packaging/version.py bump minor
python3 packaging/version.py changelog "$(python3 packaging/version.py current)"
cargo update --workspace              # Cargo.lock's copy of the version
v="v$(python3 packaging/version.py current)"
git commit -am "release: $v" && git tag -a "$v" -m "$v"
git push --atomic origin main "$v"    # the tag push starts Release
```

### Prereleases

A version with a dash in it (`0.2.0-rc.1`) is a **prerelease**: Release builds
everything and publishes a GitHub Release marked as a prerelease, and skips Homebrew,
winget, Chocolatey and apt entirely. Use one to try a release before it reaches the
package managers, and to test the pipeline after changing a workflow.

- Start one: Version bump with `minor` (or `patch`/`major`) and **pre** = `rc`
  (or `alpha`/`beta`): `0.1.0 → 0.2.0-rc.1`.
- Another: Version bump with `prerelease`: `0.2.0-rc.1 → 0.2.0-rc.2`. With a later
  label it moves on: `0.2.0-alpha.3` + `beta` → `0.2.0-beta.1`.
- Finish: Version bump with `release`: `0.2.0-rc.2 → 0.2.0`, which publishes
  everywhere.

Prereleases leave `## [Unreleased]` alone, so the notes keep collecting until the real
release and the changelog doesn't fill up with rc sections.

Check a prerelease's release page has all 11 files (6 archives, 2 `.deb`, 2 `.rpm`,
`SHA256SUMS`). An unwanted one can be deleted from the releases page, along with its tag:

```bash
git push --delete origin v0.2.0-rc.1
```

### Hotfixes

A fix for an older release, while `main` already holds unreleased features:

```bash
git switch -c release/0.2 v0.2.0       # a branch from the release being fixed
git cherry-pick <the fix>               # plus its CHANGELOG.md line under [Unreleased]
git push -u origin release/0.2
```

Then **Version bump** with *Use workflow from* = `release/0.2` and `patch`. It commits
and tags `v0.2.1` on that branch and releases it. Finally bring the changelog section
back to `main`: open a pull request from `release/0.2` into `main`, or copy the
`## [0.2.1]` section into `main`'s `CHANGELOG.md` by hand. `main`'s next release must
be newer than `0.2.1`.

### Testing without publishing

The templates and the apt repository can be tested locally at any time:

```bash
bash packaging/test.sh                # needs python3 (3.11+) with PyYAML, gpg, dpkg-deb
python3 packaging/version.py selftest
python3 packaging/check_site.py
```

### End-to-end test

**Actions → End-to-end** runs every night (and on demand, with a blueprint of your
choice). On a Linux runner with KVM it installs libvirt, builds `vmynt`, and runs
`init`, `create`, waits for the guest's bootstrap through `status --json`, then `exec`,
`snapshot`, `stop` and `remove`. On failure it uploads the VM folder, the libvirt logs,
a console screenshot and the guest's bootstrap report as an artifact. Look at its
latest run before cutting a release.

Hyper-V and Apple Virtualization can't run on GitHub's runners (no nested
virtualization), so before a release that touches them, run the same sequence by
hand on Windows and on a Mac:

```text
vmynt doctor
vmynt init --name relcheck --blueprint ubuntu-server
vmynt create
vmynt status --follow          # until bootstrap: complete
vmynt open                     # the desktop opens (desktop blueprints)
vmynt exec -- uname -a
vmynt stop && vmynt snapshot create pre && vmynt snapshot restore pre && vmynt start
vmynt remove --delete-disks
```

---

## First winget submission

Do this once, after the first **stable** GitHub Release exists, with `WINGET_TOKEN`
not yet set. You need a Windows machine with winget.

1. Download that release's `SHA256SUMS`, then render the manifests from the repository
   root (any OS with Python 3.11+):
   ```bash
   python3 packaging/render.py --sums SHA256SUMS --version 0.1.0 \
     "packaging/winget/version.yaml.tmpl=winget/{{winget_id}}.yaml" \
     "packaging/winget/installer.yaml.tmpl=winget/{{winget_id}}.installer.yaml" \
     "packaging/winget/locale.en-US.yaml.tmpl=winget/{{winget_id}}.locale.en-US.yaml"
   ```
2. On Windows, validate and test-install them:
   ```powershell
   winget validate --manifest .\winget\
   winget settings --enable LocalManifestFiles     # once, in an admin terminal
   winget install --manifest .\winget\
   vmynt --version
   winget uninstall CodeScarlett.Vmynt
   ```
3. Submit with [wingetcreate](https://github.com/microsoft/winget-create)
   (`winget install Microsoft.WingetCreate`), using the classic token from setup step 4:
   ```powershell
   wingetcreate submit --token <WINGET_TOKEN> .\winget\
   ```
   Or let [Komac](https://github.com/russellbanks/Komac) build the manifests
   interactively instead of steps 1–3:
   ```powershell
   komac new CodeScarlett.Vmynt --version 0.1.0 `
     --urls <x86_64-pc-windows-msvc.zip url> <aarch64-pc-windows-msvc.zip url> --submit
   ```
4. Wait for the pull request to be merged. Bots validate it and run the installer in a
   sandbox; a maintainer may ask questions in the PR.
5. Add the `WINGET_TOKEN` secret. From then on, every release opens the update PR
   itself.

`packaging/winget/` is only used for this step. Later releases copy the previous
version's manifests from winget-pkgs, so a change to the manifest structure (say, a
new installer switch) has to be made in a manual PR once and is then carried forward.

---

## What to expect from each channel

- **Homebrew.** The tap is yours, so there is no review: the formula is live as soon
  as the job pushes it. The formula installs the prebuilt binary and generates bash,
  zsh and fish completions by running `vmynt completion <shell>`. Getting into
  homebrew-core (plain `brew install vmynt`) is a separate application with notability
  requirements and a from-source build; the tap works without it.
- **winget.** Every version is a pull request to `microsoft/winget-pkgs`, validated by
  bots and merged by a moderator. Usually hours, sometimes a few days. Microsoft's
  SmartScreen and Defender scans run on the zip; unsigned binaries are accepted, but
  code signing reduces friction (see "Signing" in `docs/vmynt-design.md`).
- **Chocolatey.** Every version runs through automated validation and verification
  (it really installs it). The **first** version also needs a human moderator, which
  can take days to a couple of weeks. Once the package is trusted, later versions are
  often approved automatically. A version number can only ever be pushed once; to fix
  a bad package, release a new version.
- **apt.** The Pages workflow rebuilds the repository from the `.deb` files of
  **every** published non-prerelease release, so all versions stay installable
  (`apt install vmynt=1.2.3-1`). Deleting a GitHub Release removes its version from apt
  on the next deploy. Pages sites are limited to 1 GB; at roughly 1–5 MB per `.deb`
  pair that is hundreds of releases away.
- **Linux compatibility.** Linux binaries are built on Ubuntu 22.04 (glibc 2.35), so
  they run on Ubuntu 22.04+, Debian 12+ and anything newer. Older systems would need a
  musl (fully static) build target.

---

## Renaming the product later

The name is meant to be changeable, but only cheaply **before the first stable
release**. In the code and packaging it's one file:

1. In `Cargo.toml`: `name`, `[[bin]] name`, the `target/release/<name>` paths in
   `[package.metadata.deb]` and `[package.metadata.generate-rpm]` assets, and every
   field under `[package.metadata.brand]`, `repository` and `homepage`.
2. Rename the GitHub repository and the site text (`site/`, `README.md`).
3. Run `bash packaging/test.sh` and `cargo build`.

What a rename **cannot** carry over once a stable version is published:

| Published as | After a rename |
|---|---|
| winget `CodeScarlett.Vmynt` | a new package id; the old one has to be deprecated in a winget-pkgs PR |
| Chocolatey `vmynt` | a new package; the old one can be unlisted and pointed at the new one in its description |
| Homebrew `scarlettiron/tap/vmynt` | the tap can redirect with a `formula_renames.json` file |
| apt package `vmynt` | a transitional package named `vmynt` that depends on the new name |
| apt keyring `vmynt-archive-keyring.gpg` | users' `sources.list` entries point at the old url until they re-run the install lines |

---

## Rotating the apt key

Only needed if the key leaks or you want to retire it.

1. Make a new key as in setup step 6 and replace both secrets.
2. Run the Pages workflow. The repository is re-signed and the new public key is
   published at the same url.
3. Users get `NO_PUBKEY` on `apt update` until they re-run the first install line
   (the `curl ... | sudo tee /usr/share/keyrings/...`). Say so in the release notes.
4. If the old key leaked, publish its revocation certificate.

---

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| *Read Cargo.toml*: "Tag vX does not match Cargo.toml version Y" | Tag and version disagree. Delete the tag (`git push --delete origin vX`), fix one of them, tag again. |
| *Build*: `the lock file needs to be updated but --locked was passed` | `Cargo.lock` is stale or not committed. Run `cargo build`, commit `Cargo.lock`, re-tag. |
| *Build* on `windows-11-arm` or `ubuntu-22.04-arm` fails to start | Those are GitHub's standard Arm runners, free for public repositories. On a private repository they need a paid plan; make the repository public first. |
| *Homebrew*: 403 on push | `HOMEBREW_TAP_TOKEN` expired or lacks Contents write on `homebrew-tap`. |
| *winget*: package not found / cannot find existing manifests | The first version has not been submitted by hand yet. See [First winget submission](#first-winget-submission). |
| *Chocolatey*: 409 Conflict on push | That version already exists on Chocolatey (perhaps from an earlier re-run). Release a new version. |
| *Chocolatey*: test install fails | The url or hash in the rendered install script is wrong, or the zip does not contain `vmynt.exe` at its root. Check the archive and `SHA256SUMS` on the release. |
| *apt* job succeeded but `/apt/` is missing | The Pages run printed a notice: either `APT_GPG_PRIVATE_KEY` is not set or no release has a `.deb` yet. |
| Pages: "Branch is not allowed to deploy to github-pages" | Someone added `workflow_call` usage from a tag run. Run the Pages workflow on `main` instead (that's what the release does). |
| Users: `The following signatures couldn't be verified because the public key is not available` | The key was rotated, or they have an old keyring file. They re-run the `curl ... keyring` line. |
| Users: `Repository ... changed its 'Label' value` | The display name in `Cargo.toml` changed. Harmless; `sudo apt update --allow-releaseinfo-change` accepts it. |
