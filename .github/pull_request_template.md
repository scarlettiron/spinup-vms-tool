## What this changes

<!--One or two sentences. Link the issue it fixes, if there is one: "Fixes #123".-->

## Why

<!--The problem it solves, or the reason for the approach you chose.-->

## Tested on

<!--Tick every host platform you actually ran it on. Say "not applicable" for docs-only changes.-->

- [ ] Windows (Hyper-V)
- [ ] Linux (libvirt)
- [ ] macOS (vfkit)
- [ ] Not applicable (no platform-specific behavior)

Blueprint(s) used, if any:

## Checklist

- [ ] `cargo fmt` makes no changes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] New files start with the license watermark, and my name is in the `Contributors:` list of every file I changed
- [ ] The product name isn't hard-coded (`brand.rs` or `{{app}}` placeholders are used instead)
- [ ] `CHANGELOG.md` has an entry under `[Unreleased]` for anything a user would notice
- [ ] The README, `site/` and docs are updated if a command, flag, config key or blueprint changed
- [ ] No passwords, hashes, tokens, private keys or real `vm.config.local.yaml` files are included
