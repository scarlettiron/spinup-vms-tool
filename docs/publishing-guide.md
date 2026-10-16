# Publishing Vmynt, step by step

This is the walkthrough for getting Vmynt into winget, Chocolatey, Homebrew and apt
for the first time. It assumes you've never published anything to a package manager.
Every click is spelled out. [releasing.md](releasing.md) is the full reference; this
guide links to it when there's more detail.

You do the setup once, which takes an afternoon. After that, every release is one
button.

---

## What you're setting up

| Where people install from | Account you need | Cost | Who checks it | How long it takes |
|---|---|---|---|---|
| GitHub Releases (the downloads page) | GitHub (you have one) | free | nobody | minutes |
| Homebrew (`brew install`) | GitHub: a second repo called `homebrew-tap` | free | nobody, it's your repo | minutes |
| apt (`apt install` on Debian/Ubuntu) | none, just a signing key you make | free | nobody, it's your website | minutes |
| Chocolatey (`choco install`) | community.chocolatey.org | free | a robot, then a human the first time | days to a couple of weeks for the first version |
| winget (`winget install`) | GitHub: a copy (fork) of Microsoft's list | free | robots, then a Microsoft moderator | hours to a few days |
| Code signing (optional) | Azure and/or Apple | ~US$10/month Windows, US$99/year Mac | Microsoft / Apple check your ID | days |

**You don't need npm.** npm is for JavaScript packages. Vmynt doesn't use it, so that
account isn't needed for any of this. (Rust tools can be published there too, but
none of the four stores above need it.)

### Words you'll see

- **Repository (repo):** a project on GitHub. `scarlettiron/vmynt` is one.
- **Workflow / Actions:** the robots in `.github/workflows/`. GitHub runs them for you.
  The **Actions** tab on the repo shows them running.
- **Secret:** a password you give GitHub so the robots can log in somewhere for you. Once
  saved, nobody can read it back, not even you. You can only replace it.
- **Token (PAT, "personal access token"):** a password GitHub makes for robots, limited to
  certain things. Some secrets are tokens.
- **Release / tag:** a version (like `v0.2.0`) with downloadable files attached.
- **Prerelease:** a test version (like `v0.2.0-rc.1`). It goes on GitHub only, never
  to the stores.

### How to save a secret

You'll do this several times, so here it is once:

1. Open `https://github.com/scarlettiron/vmynt`.
2. **Settings** (the tab at the top right of the repo, not your profile settings).
3. Left sidebar: **Secrets and variables → Actions**.
4. Green **New repository secret** button.
5. **Name** exactly as this guide says (capitals and underscores matter). **Secret**:
   paste the value. **Add secret**.

---

## Part 1: Get the GitHub repo ready

### 1.1 Rename the repo to `vmynt`

Everything (the website, the apt address, the Homebrew formula) already says
`scarlettiron/vmynt`.

1. Repo → **Settings → General**. At the top, **Repository name** → type `vmynt` →
   **Rename**.
2. On your computer, point your copy at the new name:
   ```bash
   git remote set-url origin git@github.com:scarlettiron/vmynt.git
   ```

Do this before the first release. GitHub forwards the old repo name, but **not** the
old website address, and the apt repository lives on the website.

### 1.2 Make it public

The stores download Vmynt straight from your GitHub Releases page, so that page has to
be public. **Settings → General →** scroll to the bottom, **Danger Zone → Change
visibility → Make public**.

Everything in the git history becomes public too. Look through it first (see
[releasing.md, "Before the repository goes public"](releasing.md#0-before-the-repository-goes-public)).

### 1.3 Turn on the website (GitHub Pages)

1. **Settings → Pages**.
2. **Build and deployment → Source**: pick **GitHub Actions** (not "Deploy from a
   branch").
3. **Actions** tab → **Deploy site and apt repository** (left sidebar) → **Run
   workflow** → **Run workflow**.
4. When it goes green, `https://scarlettiron.github.io/vmynt/` is live.

---

## Part 2: Homebrew (Mac and Linux)

Homebrew lets anyone make their own mini app store (a "tap"), which is just a GitHub
repo with a particular name. Vmynt's release robot writes the install recipe (the
"formula") into it.

### 2.1 Make the tap repo

1. GitHub, top right **+ → New repository**.
2. **Owner** `scarlettiron`, **Repository name** `homebrew-tap` (exactly; the
   `homebrew-` part is how Homebrew finds it).
3. **Public**. Tick **Add a README file**. **Create repository**.

### 2.2 Make a token that can write to it

1. Your profile picture (top right) → **Settings** → bottom of the left sidebar
   **Developer settings → Personal access tokens → Fine-grained tokens → Generate new
   token**.
2. **Token name** `vmynt homebrew`. **Expiration**: a year (put a reminder in your
   calendar to make a new one).
3. **Repository access → Only select repositories →** pick `homebrew-tap`.
4. **Permissions → Repository permissions → Contents → Read and write**.
5. **Generate token**, then copy it (you only see it once).

### 2.3 Save it

Secret name: **`HOMEBREW_TAP_TOKEN`**, value: the token.

That's Homebrew done. Nobody reviews a tap, so each release is installable the moment
the robot finishes:

```bash
brew install scarlettiron/tap/vmynt
```

(Getting into Homebrew's main list so people can type just `brew install vmynt`
needs 75+ stars and a from-source build. That's for later, if at all.)

---

## Part 3: apt (Debian and Ubuntu)

apt refuses to install from a repository that isn't signed, so you make a signing key
and give the robot the private half. The robot signs the repository and publishes it
on your website at `/apt/`. There's no account to create.

You need a Linux or Mac terminal with `gpg` (Linux almost always has it; on a Mac,
`brew install gnupg`). Copy and paste this block, changing only the passphrase:

```bash
# A throwaway folder, so this key never mixes with any personal keys.
export GNUPGHOME="$(mktemp -d)"
PASS='pick-a-long-passphrase-here'

# Make the key. "never" = it doesn't expire; an expired key breaks apt for everyone.
gpg --batch --pinentry-mode loopback --passphrase "$PASS" \
    --quick-gen-key "Vmynt apt repository" rsa4096 sign never

# Save the private key and the "cancel this key" certificate to files.
gpg --pinentry-mode loopback --passphrase "$PASS" \
    --armor --export-secret-keys > apt-signing-key.asc
cp "$GNUPGHOME"/openpgp-revocs.d/*.rev apt-signing-key-revoke.asc

cat apt-signing-key.asc
```

1. Copy **everything** `cat` printed, from `-----BEGIN PGP PRIVATE KEY BLOCK-----` to
   `-----END PGP PRIVATE KEY BLOCK-----` inclusive. Save it as the secret
   **`APT_GPG_PRIVATE_KEY`**.
2. Save the passphrase as the secret **`APT_GPG_PASSPHRASE`**.
3. Put both files (`apt-signing-key.asc`, `apt-signing-key-revoke.asc`) and the
   passphrase in your password manager. Then delete them from the computer:
   ```bash
   rm -rf "$GNUPGHOME" apt-signing-key.asc apt-signing-key-revoke.asc
   unset GNUPGHOME PASS
   ```

If you lose the key, you make a new one and everyone re-downloads the public half.
Annoying, not a disaster. If someone else gets it, use the revoke file
([releasing.md, "Rotating the apt key"](releasing.md#rotating-the-apt-key)).

---

## Part 4: Chocolatey (Windows)

### 4.1 Make the account

1. Go to [community.chocolatey.org](https://community.chocolatey.org) → **Sign up**
   (top right).
2. Use **`scarlettiron`** as the username. The package says its owner is
   `scarlettiron`, so the names should match. (Using another name works if you change
   `<owners>` in `packaging/chocolatey/package.nuspec.tmpl`.)
3. Confirm your email. You can't publish until you do.

### 4.2 Get your API key

1. Signed in, open your **account** page (click your username, top right).
2. Find **API Key** and click to show it. Copy it.
3. Save it as the secret **`CHOCOLATEY_API_KEY`**.

### 4.3 What happens on the first release

You don't upload anything by hand; the robot does. Then:

1. Within about 30 minutes Chocolatey's robots check the package, install it in a test
   machine, and virus-scan it. You get emails about each step.
2. A **human moderator** looks at a new package from a new maintainer. That can take a
   few days to a couple of weeks. They may leave comments on the package page asking
   for changes. **Answer them**: a package waiting on you is rejected after about 35
   days.
3. Once it's approved, `choco install vmynt` works. Later versions go through faster.

Nobody can reserve the name `vmynt` ahead of time. Whoever's first version is approved
gets it. Right now nobody has it.

---

## Part 5: winget (Windows)

winget's catalog is one giant GitHub repo owned by Microsoft
(`microsoft/winget-pkgs`). Adding a version means opening a pull request (a "please add
this" request) against it. The robot does that for every version **except the first
one**, which you send by hand once (Part 8).

### 5.1 Copy Microsoft's repo to your account (fork it)

1. Open [github.com/microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs).
2. **Fork** (top right) → owner `scarlettiron`, keep the name `winget-pkgs` →
   **Create fork**. It's big; that's fine.

### 5.2 Make a classic token

winget's tools don't work with the newer fine-grained tokens, so this one is
"classic":

1. Profile picture → **Settings → Developer settings → Personal access tokens →
   Tokens (classic) → Generate new token → Generate new token (classic)**.
2. **Note** `vmynt winget`. **Expiration**: a year.
3. Tick **`public_repo`** and **`workflow`**. Nothing else.
4. **Generate token**, copy it.
5. Save it as the secret **`WINGET_TOKEN`**. Also keep a copy in your password manager.
   You'll paste it once more in Part 8.

---

## Part 6: Test everything with a prerelease

A prerelease runs the whole build and makes a GitHub Release marked "Pre-release",
but doesn't touch Homebrew, winget, Chocolatey or apt. It's a free rehearsal.

1. **Actions** tab → **Version bump** (left sidebar) → **Run workflow**.
2. **What to bump**: `minor`. **Prerelease label**: `rc`. Leave **Dry run** unticked.
3. **Run workflow**. It changes the version to `0.2.0-rc.1`, commits, tags, and starts
   **Release**.
4. Click into the **Release** run and watch. Six "Build" jobs (Windows, Mac and Linux,
   each on Intel and ARM) take 5 to 15 minutes. Then **GitHub Release** runs.
5. Open the repo's **Releases** page (right-hand side of the repo's front page). You
   should see `0.2.0-rc.1` with zips, tarballs, `.deb`/`.rpm` files and `SHA256SUMS`.

Download the file for your computer, unzip it, and run `vmynt --version` to check it.

The Homebrew, winget, Chocolatey and apt jobs show as **skipped**. That's correct for a
prerelease.

If something goes red, click it and read the last lines of the log. The
[troubleshooting](#when-something-goes-wrong) table below covers the usual ones.

---

## Part 7: The real release

When the prerelease looks right:

1. **Actions → Version bump → Run workflow**, **What to bump**: `release`,
   **Prerelease label**: `none`. That turns `0.2.0-rc.1` into `0.2.0`.
2. The **Release** run builds again, then runs every store job:

   | Job | What you should see |
   |---|---|
   | Homebrew | green. `Formula/vmynt.rb` appears in `homebrew-tap`. |
   | apt repository | green. It starts **Deploy site and apt repository**, which publishes `/apt/`. |
   | Chocolatey | green. You get emails from Chocolatey over the next hour (Part 4.3). |
   | winget | green with a **warning**: "not in winget-pkgs yet. Submit this first version by hand". Expected, see Part 8. |

A job that says "**… is not set, so … was skipped**" means its secret is missing or
misspelled. Fix the secret, then re-run just that release: **Actions → Release → Run
workflow →** type the tag (like `v0.2.0`) → **Run workflow**. Homebrew and apt skip
anything already done. Chocolatey refuses a version it already has (a 409 error,
harmless). winget may open a second pull request; close the extra one.

---

## Part 8: The first winget version, by hand (once ever)

The repo already has the three manifest files winget wants, as templates in
`packaging/winget/`. You fill them in with the release's numbers, test them, and send
them in.

**On any computer with Python** (from the repo folder), fill in the templates:

1. From the release's page (**Releases →** your version), download `SHA256SUMS` into
   the repo folder.
2. Run this, with your real version number:
   ```bash
   python3 packaging/render.py --sums SHA256SUMS --version 0.2.0 \
     "packaging/winget/version.yaml.tmpl=winget/{{winget_id}}.yaml" \
     "packaging/winget/installer.yaml.tmpl=winget/{{winget_id}}.installer.yaml" \
     "packaging/winget/locale.en-US.yaml.tmpl=winget/{{winget_id}}.locale.en-US.yaml"
   ```
   That makes a `winget` folder with three `CodeScarlett.Vmynt…yaml` files. Copy the
   folder to the Windows computer if you're not on it.

**On Windows**, in PowerShell, in the folder that holds `winget`:

```powershell
# 1. Check the files are valid.
winget validate --manifest .\winget\

# 2. Try installing from them. The settings line is needed once, in an
#    administrator PowerShell.
winget settings --enable LocalManifestFiles
winget install --manifest .\winget\
vmynt --version
winget uninstall CodeScarlett.Vmynt

# 3. Install Microsoft's submission tool and send them in, with the classic token
#    from Part 5.2.
winget install Microsoft.WingetCreate
wingetcreate submit --token PASTE-YOUR-CLASSIC-TOKEN .\winget\
```

`wingetcreate` opens the pull request against Microsoft's repo and prints its link.

Then:

- Bots comment on the pull request within an hour. They test-install it in a sandbox.
  A label like **`Azure-Pipeline-Passed`** is good news.
- A Microsoft moderator merges it, usually in hours to a few days.
- If they add **`Needs-Author-Feedback`**, reply in the pull request within 10 days or
  it's closed.
- If **`Validation-Defender-Error`** or **`Binary-Validation-Error`** appears,
  Microsoft's antivirus flagged the unsigned exe. It happens to new, unsigned
  programs. Comment on the pull request and ask, or submit the exe as a false positive
  at
  [microsoft.com/wdsi/filesubmission](https://www.microsoft.com/en-us/wdsi/filesubmission).
  Code signing (Part 11) makes this much less likely.

Once it's merged, `winget install CodeScarlett.Vmynt` works, and **every later release
updates winget automatically**.

[Komac](https://github.com/russellbanks/Komac) (`komac new`) is another way to build
and submit the first version, by answering questions instead of using the templates;
see [releasing.md, "First winget submission"](releasing.md#first-winget-submission).

---

## Part 9: Check each store really works

Give each store its review time, then try it on a real computer:

```powershell
# Windows
winget install CodeScarlett.Vmynt
choco install vmynt          # in an administrator terminal
vmynt --version
```

```bash
# Mac or Linux with Homebrew
brew install scarlettiron/tap/vmynt
vmynt --version

# Debian / Ubuntu
curl -fsSL https://scarlettiron.github.io/vmynt/apt/vmynt-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/vmynt-archive-keyring.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/vmynt-archive-keyring.gpg] https://scarlettiron.github.io/vmynt/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/vmynt.list
sudo apt update && sudo apt install vmynt
vmynt --version
```

Each should print the version you released.

---

## Part 10: Every release after that

1. Make sure `CHANGELOG.md` has your changes under `## [Unreleased]`.
2. **Actions → Version bump → Run workflow**:
   - bug fixes only → `patch` (0.2.0 → 0.2.1)
   - new features → `minor` (0.2.1 → 0.3.0)
   - the big one → `major` (0.x → 1.0.0)
   - want a rehearsal first? any of those with a **Prerelease label** like `rc`, then
     `release` once it's good
3. That's it. Homebrew and apt update in minutes. winget opens its own pull request and
   it merges within a day or two. Chocolatey approves it after its robots pass.

Tick **Dry run** to see what a bump would do without changing anything.

**1.0.0** is the "this is stable" promise. Hold it until Vmynt has been run for real on
Windows (Hyper-V) and a Mac. Until 1.0, a `minor` version is allowed to change how
things work.

---

## Part 11 (optional): Code signing

Without signing everything still installs. The differences:

- **Windows:** winget and Chocolatey install unsigned programs fine. People who
  download the zip **in a browser** get a blue "Windows protected your PC" box and
  have to click **More info → Run anyway**. Microsoft's antivirus is also more
  suspicious of unsigned programs (see Part 8).
- **Mac:** Homebrew installs work fine unsigned. People who download the tarball **in a
  browser** get "Apple could not verify…" and have to allow it in **System Settings →
  Privacy & Security**.

To sign:

- **Windows: Azure Artifact Signing**, about US$10 a month. Individuals must live in the
  US or Canada. You verify your identity with a government ID in the Azure portal.
- **Windows, if you're outside the US/Canada:** [SignPath Foundation](https://signpath.org/)
  signs open-source projects free, but each release needs their approval and the
  signature says "SignPath Foundation" rather than your name. It needs a different
  workflow step; ask for it if you go this way.
- **Mac: Apple Developer Program**, US$99 a year. You make a "Developer ID
  Application" certificate and an API key for notarization (Apple's malware check).

The click-by-click setup and the secret names are in
[releasing.md, "Code signing"](releasing.md#7-code-signing-optional--azure_-apple_). Once
the secrets are saved, the next release is signed automatically.

---

## Part 12 (optional): crates.io (`cargo install vmynt`)

Rust developers can install straight from crates.io, Rust's package site. It isn't
wired into the release robot, and it's optional.

1. Sign in at [crates.io](https://crates.io) with GitHub and verify your email
   (**Account Settings**).
2. **Account Settings → API Tokens → New Token**, scopes **publish-new** and
   **publish-update**. Copy it.
3. On your computer, from the repo:
   ```bash
   cargo login            # paste the token
   cargo publish --dry-run
   cargo publish
   ```

A published version can never be deleted, only "yanked" (hidden from new installs), so
do this only for a version you're happy with. After the first one, crates.io's
**Trusted Publishing** lets GitHub Actions publish without a token. Ask if you want that
wired up.

---

## When something goes wrong

| What you see | What it means | Fix |
|---|---|---|
| A store job says "**X is not set, so … was skipped**" | that secret is missing or its name is misspelled | add/rename the secret, then **Actions → Release → Run workflow** with the tag |
| Homebrew job: **403** or "Permission denied" | the tap token expired or can't write to `homebrew-tap` | make a new fine-grained token (Part 2.2), replace `HOMEBREW_TAP_TOKEN` |
| winget job: warning "**not in winget-pkgs yet**" | the first version hasn't been merged | do Part 8 |
| winget job: "Bad credentials" / 401 | the classic token expired | new classic token (Part 5.2), replace `WINGET_TOKEN` |
| Chocolatey push: **409 Conflict** | that version is already on Chocolatey | nothing to do; release a new version to change anything |
| Chocolatey push: **403** | wrong API key, or someone else owns the `vmynt` name | check the key on your account page |
| apt: users get "**NO_PUBKEY**" or "**not signed**" | the Pages deploy didn't run, or the key secret is wrong | **Actions → Deploy site and apt repository → Run workflow**; check `APT_GPG_PRIVATE_KEY` is the whole block |
| **Release** fails at "Check the tag matches the version" | a tag was pushed by hand with a different number than `Cargo.toml` | delete that tag; use **Version bump** instead |
| **Version bump** fails pushing to `main` | `main` has branch protection | see `RELEASE_TOKEN` in [releasing.md](releasing.md#every-secret) |

More in [releasing.md, "Troubleshooting"](releasing.md#troubleshooting).
