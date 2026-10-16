# winget manifest templates

These are only needed **once**, for the first version ever submitted to
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). After that
first pull request is merged, the release workflow updates the package by itself
(it copies the previous version's manifests and swaps in the new urls and hashes).

They are not watermarked like the rest of the repository on purpose: they are
submitted to Microsoft's repository, where only the manifest fields belong.

See "First winget submission" in [docs/releasing.md](../../docs/releasing.md) for
the steps.
