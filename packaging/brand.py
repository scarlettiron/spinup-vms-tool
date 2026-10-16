#!/usr/bin/env python3
#Vmseed
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)

"""Reads the product's name, ids and urls out of Cargo.toml.

Cargo.toml is the one place the product name lives. Every workflow and packaging
script asks this file for it instead of typing the name out, so renaming the
product means editing Cargo.toml and nothing here.

Usage:
    python3 packaging/brand.py                 print every value as KEY=value lines
    python3 packaging/brand.py --json          print every value as JSON
    python3 packaging/brand.py --get name      print one value
    python3 packaging/brand.py --github-output append every value to $GITHUB_OUTPUT
    python3 packaging/brand.py --manifest path read a different Cargo.toml
                                               (or set BRAND_MANIFEST)

Needs Python 3.11 or newer, because that is when tomllib joined the standard
library. Nothing has to be installed.
"""

import argparse
import json
import os
import re
import sys
import tomllib
from pathlib import Path

#The repository root is one folder up from this script.
ROOT = Path(__file__).resolve().parent.parent

#Which Cargo.toml to read. BRAND_MANIFEST points somewhere else, which the
#packaging tests use to run against a stand-in file.
DEFAULT_MANIFEST = Path(os.environ.get("BRAND_MANIFEST", ROOT / "Cargo.toml"))

#Keys that must be present under [package.metadata.brand] in Cargo.toml.
#They are copied to the output with dashes turned into underscores.
REQUIRED_BRAND_KEYS = (
    "display-name",
    "winget-id",
    "winget-publisher",
    "chocolatey-id",
    "homebrew-tap",
    "site-url",
    "apt-url",
)


def load(manifest: Path) -> dict[str, str]:
    """Returns every value the packaging needs, as a flat dict of strings."""

    with manifest.open("rb") as handle:
        cargo = tomllib.load(handle)

    package = cargo["package"]
    brand = package.get("metadata", {}).get("brand", {})

    missing = [key for key in REQUIRED_BRAND_KEYS if key not in brand]
    if missing:
        raise SystemExit(
            f"{manifest}: [package.metadata.brand] is missing {', '.join(missing)}"
        )

    values: dict[str, str] = {
        "name": package["name"],
        "version": package["version"],
        "description": package["description"],
        "license": package["license"],
        "repository": package["repository"].rstrip("/"),
        "homepage": package["homepage"],
        "authors": ", ".join(package.get("authors", [])),
    }

    for key in REQUIRED_BRAND_KEYS:
        values[key.replace("-", "_")] = str(brand[key])

    #Optional overrides. Each one has a sensible default worked out below.
    for key in ("homebrew-desc", "icon-url"):
        if key in brand:
            values[key.replace("-", "_")] = str(brand[key])

    #"owner/repo", taken from the repository url. The GitHub Release download
    #links and the gh commands all need this form.
    match = re.match(r"https://github\.com/([^/]+/[^/]+?)(?:\.git)?$", values["repository"])
    if not match:
        raise SystemExit(f"repository is not a github.com url: {values['repository']}")
    values["repo_slug"] = match.group(1)
    values["repo_owner"] = values["repo_slug"].split("/")[0]

    #Homebrew wants the formula class in CamelCase: "my-tool" -> "MyTool".
    values["formula_class"] = "".join(
        part[:1].upper() + part[1:] for part in re.split(r"[-_]", values["name"]) if part
    )

    #Homebrew's own style check asks for a short description with no full stop.
    #Take the part before an em dash, unless Cargo.toml gives one explicitly.
    if "homebrew_desc" not in values:
        short = values["description"].split(" — ")[0].strip().rstrip(".")
        values["homebrew_desc"] = short

    #The tap repository is "owner/homebrew-tap", but users type "owner/tap".
    tap_owner, tap_repo = values["homebrew_tap"].split("/")
    values["homebrew_tap_short"] = f"{tap_owner}/{tap_repo.removeprefix('homebrew-')}"

    #The apt keyring file is named after the product.
    values["apt_keyring"] = f"{values['name']}-archive-keyring"

    #winget ids look like "Publisher.Package". The manifests folder uses both halves.
    winget_publisher_part, _, winget_package_part = values["winget_id"].partition(".")
    values["winget_id_publisher"] = winget_publisher_part
    values["winget_id_package"] = winget_package_part

    return values


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--json", action="store_true")
    group.add_argument("--get", metavar="KEY")
    group.add_argument("--github-output", action="store_true")
    args = parser.parse_args()

    values = load(args.manifest)

    if args.get:
        if args.get not in values:
            raise SystemExit(f"unknown key {args.get!r}; known keys: {', '.join(sorted(values))}")
        print(values[args.get])
    elif args.json:
        print(json.dumps(values, indent=2))
    elif args.github_output:
        #GitHub Actions reads step outputs from the file named in $GITHUB_OUTPUT.
        target = os.environ.get("GITHUB_OUTPUT")
        if not target:
            raise SystemExit("--github-output needs $GITHUB_OUTPUT (run it inside GitHub Actions)")
        with open(target, "a", encoding="utf-8") as handle:
            for key, value in values.items():
                handle.write(f"{key}={value}\n")
        for key, value in values.items():
            print(f"{key}={value}")
    else:
        for key, value in values.items():
            print(f"{key}={value}")


if __name__ == "__main__":
    try:
        main()
    except FileNotFoundError as error:
        sys.exit(f"brand.py: {error}")
