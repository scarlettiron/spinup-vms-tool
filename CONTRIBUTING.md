# Contributing to Vmynt

Thanks for helping. Vmynt is young and has one maintainer, so small, focused pull requests
are the easiest to review and the quickest to land. If you're planning something big, open an
issue or a discussion first so we can agree on the shape before you write the code.

The plan for the whole project lives in [docs/vmynt-design.md](docs/vmynt-design.md). Read the
part that touches your change before you start. It explains why things are the way they are.

## Code conventions

These rules are partly enforced by `cargo fmt` and `cargo clippy`, and partly by review.

### Comments

- Write comments so that people with less software experience can follow them. This is so
  beginner engineers contributing will be able to decipher the code more easily.
- Write line comments with no space after the comment marker: `//Like this`, `#Like this`.
- Explain _why_ a piece of code exists, not just what it does. If a line looks odd but is
  deliberate (a workaround, a quirk of a hypervisor, an installer that is fussy about a label),
  say so next to it.

### Layout and style

- Keep logic readable with a reasonable amount of blank lines between blocks.
- Put values that could be reused anywhere into a constants file under a `constants` folder
  (`src/constants/`).
- Put reusable helpers into a utilities file under a `utilities` folder (`src/utilities/`).
- Prefer small structs with methods, or plain functions, over deep trait hierarchies.
- Separate things into appropriate folders. Each CLI command gets its own module under
  `src/cli/`, and each hypervisor backend its own module under `src/hypervisor/`.
- Everything must be strictly typed. Parse config into typed structs or enums as early as you
  can, and avoid passing loose strings or untyped YAML values around.
- Return errors instead of panicking. `unwrap()` and `expect()` are only for things that truly
  cannot fail, with a comment saying why.

### Never hard-code the product name

The name Vmynt is expected to change, so nothing may spell it out by hand:

- In Rust, use `brand::NAME`, `brand::DISPLAY_NAME` and `brand::DIR` from `src/brand.rs`.
- In embedded templates, help text and generated files, use the `{{app}}`, `{{App}}` and
  `{{dir}}` placeholders, which are filled in at runtime.
- The source of truth is `Cargo.toml`: `name` under `[package]`, and the display name and
  package manager IDs under `[package.metadata.brand]`. The release workflows read them from
  there too.

The license watermark (below), the website, and the docs are the exceptions. They name the
product in plain text and get updated with a search and replace on a rename.

## License watermark

Vmynt is published under the MIT License. See the [LICENSE](LICENSE) file for the full text.

Every source file must start with the license watermark. It names the license and lists the
people who have worked on the file. In Rust, JavaScript and CSS it looks like this:

```rust
//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)
```

Use the comment syntax of the file you're in:

| File type                                  | Watermark style                                                          |
| ------------------------------------------ | ------------------------------------------------------------------------ |
| Rust (`.rs`), JavaScript (`.js`)            | `//` on every line, as above                                             |
| CSS (`.css`)                                | one `/* … */` block holding the same lines                               |
| Bash (`.sh`), PowerShell (`.ps1`, `.psm1`), YAML (`.yml`, `.yaml`), TOML | `#` on every line                                                        |
| HTML (`.html`)                              | one `<!-- … -->` block, placed right after `<!DOCTYPE html>`             |
| Markdown                                    | no watermark needed                                                      |

For example, in a bash script:

```bash
#!/usr/bin/env bash
#Vmynt
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)
```

Rules for the watermark:

- Do not remove or change the license line or the copyright line.
- If a file has a shebang like `#!/usr/bin/env bash`, keep the shebang on the first line and put
  the watermark right below it. The same goes for anything that must be first in a file, such as
  `<!DOCTYPE html>` or a PowerShell `#requires` line.
- When you edit a file, add your name to the bottom of the `Contributors:` list in that file. Add
  it after the names that are already there, so the order shows who worked on the file first.
  Keep Scarlett A. Scott (codescarlett) at the top of the list.
- Use the format `Full Name (handle)`, one contributor per line.
- Only add your name once per file. If it is already in the list, leave it as it is.
- Some older scripts from the PowerShell tool don't have a watermark yet. If you edit one, add
  the watermark with both names.

For example, after a second person edits a file, the contributors section would look like this:

```rust
//Contributors:
//Scarlett A. Scott (codescarlett)
//Jordan Lee (jlee)
```

## Tooling

### Getting set up

1. Install Rust with [rustup](https://rustup.rs). The repository pins nothing beyond the
   `rust-version` in `Cargo.toml`, so the current stable toolchain works.
2. Clone the repository and build:

   ```bash
   git clone https://github.com/scarlettiron/vmynt.git
   cd vmynt
   cargo build
   cargo run -- --help
   ```

You do not need Hyper-V, KVM or vfkit to work on most of the code. Config loading, blueprints,
templates, the seed disk, and the CLI itself are all tested against an in-memory fake backend.

### Before you open a pull request

Run all three, and fix anything they report:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same commands on Windows, macOS and Linux, so a pull request that passes locally on
one platform can still fail on another. If it does, the CI log says which.

## How the code is laid out

[docs/vmynt-design.md §5](docs/vmynt-design.md#5-architecture) has the full source layout. The
short version:

- `src/cli/` holds one module per command (`init`, `create`, `list`, …).
- `src/hypervisor/` holds the `Hypervisor` trait and one backend per host OS (Hyper-V on
  Windows, libvirt on Linux, vfkit on macOS). Each backend only compiles on its own OS.
- `src/config/`, `src/blueprint/`, `src/seed/` and the rest are shared by every platform and
  contain no OS-specific code.
- `src/brand.rs` is the only place the product name lives in code.

### Guest scripts run inside the VM

The bash scripts under `blueprints/bootstrap/` and each blueprint's own `bootstrap/` folder do
**not** run on your machine. They're copied onto the seed disk and run inside the new VM on its
first boot, as root. That means:

- They must work on every distribution the blueprint covers (`dnf` and `apt` families), so use
  the helpers in `blueprints/bootstrap/_lib.sh` rather than calling a package manager directly.
- They must be safe to re-run. A step that fails part way gets retried on the next boot.
- They can't assume anything about the host. Settings reach them as `VM_*` environment
  variables written by the seed disk.
- Keep them with LF line endings. `.gitattributes` enforces this, and a CRLF script fails inside
  the guest with confusing errors.

### Adding a blueprint

A blueprint is one operating system and how to install it unattended.

1. Copy the closest existing folder under `blueprints/`, for example
   `blueprints/redhat-rocky-workstation/`, and rename it `<family>-<distro>-<flavour>`.
2. Update its `blueprint` file: name, description, OS version pattern, install media URLs and
   checksum file (one entry per architecture you support), and the provisioning method.
3. Update the seed template under `seed/`. Templates use `{{PLACEHOLDER}}` substitution, nothing
   more.
4. Reuse the shared bootstrap steps. Only add a blueprint-specific step under its own
   `bootstrap/` folder if the shared one genuinely can't handle the OS.
5. Build a VM with it on at least one host platform, and say which in the pull request.

[docs/adding-an-os.md](docs/adding-an-os.md) walks through this in more detail. Some of it
describes the PowerShell tool's file format, which Vmynt is moving away from (see
[the design](docs/vmynt-design.md#9-blueprints)).

## The website

The project website lives in `site/` and is published to GitHub Pages. Its look comes entirely
from the `site/theme/` folder, which has its own README explaining the markup each piece
expects. Keep pages free of inline styles so the theme stays portable.

If your change adds or renames a command, a config key, or a blueprint, update the website and
the README in the same pull request.

## Commits and pull requests

- Keep each pull request to one change. Two unrelated fixes are two pull requests.
- Write commit messages in the imperative ("Add vfkit snapshot support"), with a short first
  line and a body explaining why if it isn't obvious.
- Fill in the pull request template. It asks which host platform you tested on.
- Add your name to the `Contributors:` list of every file you changed.
- Add an entry under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for anything a user would
  notice: a new command or flag, a changed default, a fixed bug. Internal refactors don't need
  one.
- Don't include generated files, build output, or anything from your own `.vmynt/` folder.
  Never commit a real `vm.config.local.yaml`, password, token or private key.

## Reporting security problems

Please don't open a public issue. See [SECURITY.md](SECURITY.md).

## Code of conduct

Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
