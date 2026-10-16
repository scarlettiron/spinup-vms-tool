#!/usr/bin/env python3
#Vmynt
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)

"""Fills in the package manager templates for one release.

A template is an ordinary file with {{placeholders}} in it. This script swaps
each placeholder for a real value: the product name and ids from Cargo.toml
(see brand.py), the version, and a download url and sha256 for every file
listed in the release's SHA256SUMS.

Usage:
    python3 packaging/render.py --sums SHA256SUMS TEMPLATE=OUTPUT [TEMPLATE=OUTPUT ...]

Output paths may contain placeholders too, which is how the winget manifests get
their required "<Id>.installer.yaml" style names:

    packaging/winget/installer.yaml.tmpl=out/{{winget_id}}.installer.yaml

Placeholders made from SHA256SUMS, for a product called "tool", version 1.2.3:

    tool-1.2.3-x86_64-apple-darwin.tar.gz -> {{url_x86_64_apple_darwin}}  {{sha256_x86_64_apple_darwin}}
    tool_1.2.3_amd64.deb                  -> {{url_deb_amd64}}            {{sha256_deb_amd64}}
    tool-1.2.3-1.x86_64.rpm               -> {{url_rpm_x86_64}}           {{sha256_rpm_x86_64}}

Run with --list to see every placeholder and its value. A template that uses a
placeholder with no value is an error, so a missing download can never turn into
an empty url in a published package.
"""

import argparse
import datetime
import re
import sys
from pathlib import Path

#brand.py sits in the same folder. Importing it should not leave a
#__pycache__ folder behind in the repository.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import brand  # noqa: E402

PLACEHOLDER = re.compile(r"\{\{\s*([a-z0-9_]+)\s*\}\}")


def asset_values(name: str, version: str, base_url: str, sums: Path) -> dict[str, str]:
    """Turns each line of SHA256SUMS into a url_* and a sha256_* placeholder."""

    values: dict[str, str] = {}
    archive = re.compile(rf"^{re.escape(name)}-{re.escape(version)}-(.+)\.(tar\.gz|zip)$")
    deb = re.compile(rf"^{re.escape(name)}_{re.escape(version)}(?:-[^_]+)?_([a-z0-9]+)\.deb$")
    rpm = re.compile(rf"^{re.escape(name)}-{re.escape(version)}-[^.]+\.([a-z0-9_]+)\.rpm$")

    for line in sums.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue

        #sha256sum writes "<hash>  <file>", or "<hash> *<file>" in binary mode.
        digest, _, filename = line.partition(" ")
        filename = filename.strip().lstrip("*")

        if match := archive.match(filename):
            key = match.group(1).replace("-", "_")
        elif match := deb.match(filename):
            key = f"deb_{match.group(1)}"
        elif match := rpm.match(filename):
            key = f"rpm_{match.group(1)}"
        else:
            #Anything else in the release (the sums file itself, notes, ...) is ignored.
            continue

        values[f"url_{key}"] = f"{base_url}/{filename}"
        values[f"sha256_{key}"] = digest.lower()

    return values


def build_values(args: argparse.Namespace) -> dict[str, str]:
    values = brand.load(args.manifest)
    if args.version:
        values["version"] = args.version.removeprefix("v")

    values["tag"] = f"v{values['version']}"
    values["release_url"] = f"https://github.com/{values['repo_slug']}/releases/download/{values['tag']}"
    values["release_page"] = f"https://github.com/{values['repo_slug']}/releases/tag/{values['tag']}"
    values["date"] = datetime.date.today().isoformat()

    if args.sums:
        values.update(asset_values(values["name"], values["version"], values["release_url"], args.sums))

    return values


def fill(text: str, values: dict[str, str], where: str) -> str:
    unknown = sorted({key for key in PLACEHOLDER.findall(text) if key not in values})
    if unknown:
        raise SystemExit(
            f"render.py: {where} uses placeholders with no value: {', '.join(unknown)}\n"
            "Run with --list to see the ones that exist."
        )
    return PLACEHOLDER.sub(lambda match: values[match.group(1)], text)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--manifest", type=Path, default=brand.DEFAULT_MANIFEST)
    parser.add_argument("--sums", type=Path, help="the release's SHA256SUMS file")
    parser.add_argument("--version", help="override the Cargo.toml version (a leading v is fine)")
    parser.add_argument("--list", action="store_true", help="print every placeholder and exit")
    parser.add_argument("pairs", nargs="*", metavar="TEMPLATE=OUTPUT")
    args = parser.parse_args()

    values = build_values(args)

    if args.list:
        for key in sorted(values):
            print(f"{{{{{key}}}}} = {values[key]}")
        return

    if not args.pairs:
        parser.error("give at least one TEMPLATE=OUTPUT pair, or --list")

    for pair in args.pairs:
        template, sep, output = pair.partition("=")
        if not sep:
            parser.error(f"expected TEMPLATE=OUTPUT, got {pair!r}")

        source = Path(template)
        target = Path(fill(output, values, f"output path {output!r}"))
        rendered = fill(source.read_text(encoding="utf-8"), values, str(source))

        target.parent.mkdir(parents=True, exist_ok=True)
        #newline="" writes the template's line endings exactly as they are.
        with target.open("w", encoding="utf-8", newline="") as handle:
            handle.write(rendered)
        print(f"rendered {source} -> {target}")


if __name__ == "__main__":
    main()
