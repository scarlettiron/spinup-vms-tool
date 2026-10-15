#!/usr/bin/env python3
#Vmynt
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)

"""Reads and changes the version, which lives in one place: Cargo.toml.

Every other place that shows a version (the binary, the packages, the release
notes, the website's install page) reads it from Cargo.toml, so this script is
the only thing that ever writes it.

Usage:
    python3 packaging/version.py current
        print the version, for example 0.2.0-rc.1

    python3 packaging/version.py bump patch|minor|major|prerelease|release [--pre ID]
        change the version in Cargo.toml and print the new one
          patch       0.1.0 -> 0.1.1      0.2.0-rc.1 -> 0.2.0
          minor       0.1.3 -> 0.2.0
          major       0.4.1 -> 1.0.0
          prerelease  0.1.0 -> 0.1.1-rc.1  0.1.1-rc.1 -> 0.1.1-rc.2
                      (--pre picks the label: alpha, beta or rc; default rc.
                       Moving to a later label restarts the count:
                       0.2.0-alpha.3 --pre beta -> 0.2.0-beta.1)
          release     0.2.0-rc.2 -> 0.2.0  (drops the prerelease label)
        --pre with patch/minor/major makes the bumped version a prerelease:
          minor --pre alpha   0.1.3 -> 0.2.0-alpha.1

    python3 packaging/version.py check-tag v0.2.0
        exit 1 unless the tag is exactly "v" + the Cargo.toml version

    python3 packaging/version.py changelog 0.2.0 [--date YYYY-MM-DD]
        move the "## [Unreleased]" notes in CHANGELOG.md under a new
        "## [0.2.0] - date" heading, and update the links at the bottom

    python3 packaging/version.py notes 0.2.0
        print the CHANGELOG.md section for one version (the release notes)

    python3 packaging/version.py selftest
        run this script's own tests

Options for every command: --manifest PATH (a different Cargo.toml) and
--changelog PATH (a different CHANGELOG.md).

Needs Python 3.11 or newer (for tomllib). Nothing has to be installed.
"""

import argparse
import datetime
import os
import re
import sys
import tempfile
import tomllib
from pathlib import Path

#The repository root is one folder up from this script.
ROOT = Path(__file__).resolve().parent.parent

#A semantic version: 1.2.3, optionally followed by -label.number (1.2.3-rc.1).
#Build metadata (+abc) is not used by this project, so it is not accepted.
VERSION_RE = re.compile(
    r"^(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)"
    r"(?:-(?P<pre>[0-9A-Za-z.-]+))?$"
)

#Prerelease labels in the order they come, so moving from alpha to beta to rc
#always goes forwards.
PRE_LABELS = ("alpha", "beta", "rc")

#The line in Cargo.toml's [package] table that holds the version.
VERSION_LINE_RE = re.compile(r'^(?P<lead>\s*version\s*=\s*")(?P<version>[^"]*)(?P<tail>".*)$')


class VersionError(Exception):
    """A problem worth a one-line message rather than a stack trace."""


#------------------------------------------------------------------- versions


def parse(version: str) -> tuple[int, int, int, str | None]:
    """Splits "1.2.3-rc.1" into (1, 2, 3, "rc.1")."""

    match = VERSION_RE.match(version.strip())
    if not match:
        raise VersionError(f"'{version}' is not a version like 1.2.3 or 1.2.3-rc.1")
    return (
        int(match["major"]),
        int(match["minor"]),
        int(match["patch"]),
        match["pre"],
    )


def split_pre(pre: str) -> tuple[str, int]:
    """Splits a prerelease label "rc.2" into ("rc", 2). A label with no number counts as 0."""

    label, _, number = pre.partition(".")
    if number and not number.isdigit():
        raise VersionError(f"prerelease '{pre}' should look like rc.1, beta.2 or alpha.3")
    return label, int(number or 0)


def bump(version: str, part: str, pre: str | None = None) -> str:
    """Returns the version after a bump. See the usage at the top of this file."""

    major, minor, patch, current_pre = parse(version)
    if pre is not None and pre not in PRE_LABELS:
        raise VersionError(f"--pre must be one of {', '.join(PRE_LABELS)}, not '{pre}'")

    if part == "release":
        if current_pre is None:
            raise VersionError(f"{version} is already a release; use patch, minor or major")
        return f"{major}.{minor}.{patch}"

    if part == "prerelease":
        label = pre or "rc"
        if current_pre is None:
            #A prerelease of the next patch: 0.1.0 -> 0.1.1-rc.1
            return f"{major}.{minor}.{patch + 1}-{label}.1"
        current_label, number = split_pre(current_pre)
        if label == current_label:
            return f"{major}.{minor}.{patch}-{label}.{number + 1}"
        if current_label in PRE_LABELS and PRE_LABELS.index(label) < PRE_LABELS.index(current_label):
            raise VersionError(
                f"{version} is already past {label}; a prerelease can only move forwards "
                f"({' -> '.join(PRE_LABELS)})"
            )
        return f"{major}.{minor}.{patch}-{label}.1"

    if part == "patch":
        #A prerelease of x.y.z becomes x.y.z itself: 0.2.0-rc.1 -> 0.2.0
        new = (major, minor, patch) if current_pre and pre is None else (major, minor, patch + 1)
    elif part == "minor":
        same = current_pre and patch == 0 and pre is None
        new = (major, minor, 0) if same else (major, minor + 1, 0)
    elif part == "major":
        same = current_pre and minor == 0 and patch == 0 and pre is None
        new = (major, 0, 0) if same else (major + 1, 0, 0)
    else:
        raise VersionError(f"unknown bump '{part}'; use patch, minor, major, prerelease or release")

    text = ".".join(str(n) for n in new)
    return f"{text}-{pre}.1" if pre else text


#------------------------------------------------------------------ Cargo.toml


def read_version(manifest: Path) -> str:
    """The [package] version from Cargo.toml."""

    with manifest.open("rb") as handle:
        cargo = tomllib.load(handle)
    try:
        version = cargo["package"]["version"]
    except KeyError:
        raise VersionError(f"{manifest} has no [package] version") from None
    parse(version)
    return version


def write_version(manifest: Path, new_version: str) -> None:
    """Changes the version line inside [package], and nothing else in the file."""

    parse(new_version)
    lines = manifest.read_text(encoding="utf-8").splitlines(keepends=True)
    section = None
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("[") and not stripped.startswith("[["):
            section = stripped.strip("[]").strip()
            continue
        if stripped.startswith("[["):
            section = None
            continue
        if section != "package":
            continue
        match = VERSION_LINE_RE.match(line.rstrip("\r\n"))
        if match:
            ending = line[len(line.rstrip("\r\n")):]
            lines[index] = f"{match['lead']}{new_version}{match['tail']}{ending}"
            manifest.write_text("".join(lines), encoding="utf-8")
            if read_version(manifest) != new_version:
                raise VersionError("the version line was changed but Cargo.toml reads differently")
            return
    raise VersionError(f'no version = "..." line found in the [package] table of {manifest}')


#---------------------------------------------------------------- CHANGELOG.md

#"## [0.2.0] - 2026-10-13", "## [Unreleased]", or a plain "## 0.2.0".
HEADING_RE = re.compile(r"^## \[?(?P<name>[^\]\s]+)\]?")
#"[0.2.0]: https://..." link definitions at the bottom.
LINK_RE = re.compile(r"^\[(?P<name>[^\]]+)\]:\s*(?P<url>\S+)\s*$")


def section_bounds(lines: list[str], name: str) -> tuple[int, int] | None:
    """The line range of one "## [name]" section: heading line, and the line after its end."""

    start = None
    for index, line in enumerate(lines):
        heading = HEADING_RE.match(line)
        if heading:
            if start is not None:
                return start, index
            if heading["name"].lower() == name.lower():
                start = index
        elif start is not None and LINK_RE.match(line):
            return start, index
    if start is None:
        return None
    return start, len(lines)


def notes(changelog: Path, version: str) -> str:
    """The body of the section for one version, without its heading."""

    lines = changelog.read_text(encoding="utf-8").splitlines()
    bounds = section_bounds(lines, version)
    if bounds is None:
        raise VersionError(f"CHANGELOG.md has no '## [{version}]' section")
    body = lines[bounds[0] + 1 : bounds[1]]
    return "\n".join(body).strip() + "\n"


def repository_url(lines: list[str]) -> str | None:
    """The https://github.com/owner/repo the existing links point at."""

    for line in lines:
        link = LINK_RE.match(line)
        if link:
            match = re.match(r"(https://github\.com/[^/]+/[^/]+)/", link["url"])
            if match:
                return match.group(1)
    return None


def release_changelog(changelog: Path, version: str, date: str) -> None:
    """Moves the Unreleased notes under a new heading for `version`."""

    parse(version)
    text = changelog.read_text(encoding="utf-8")
    lines = text.splitlines()

    if section_bounds(lines, version) is not None:
        raise VersionError(f"CHANGELOG.md already has a section for {version}")
    bounds = section_bounds(lines, "Unreleased")
    if bounds is None:
        raise VersionError("CHANGELOG.md has no '## [Unreleased]' section to release")

    start, end = bounds
    body = [line for line in lines[start + 1 : end]]
    #Trailing blank lines belong between sections, not inside this one.
    while body and not body[-1].strip():
        body.pop()
    if not any(line.strip() for line in body):
        body = ["", "No user-facing changes."]

    new_section = ["## [Unreleased]", "", f"## [{version}] - {date}"] + body + [""]
    lines = lines[:start] + new_section + lines[end:]

    #Links at the bottom: Unreleased compares from the new tag, and the new
    #version compares from the previous one (or points at its tag if it is the first).
    repo = repository_url(lines)
    if repo:
        previous = None
        for line in lines:
            heading = HEADING_RE.match(line)
            if heading and heading["name"] not in ("Unreleased", version):
                previous = heading["name"]
                break
        version_url = (
            f"{repo}/compare/v{previous}...v{version}" if previous else f"{repo}/releases/tag/v{version}"
        )
        unreleased_url = f"{repo}/compare/v{version}...HEAD"
        kept = [line for line in lines if not (LINK_RE.match(line) and LINK_RE.match(line)["name"] in ("Unreleased", version))]
        #Put the two new links where the old Unreleased link was (the top of the list).
        first_link = next((i for i, line in enumerate(kept) if LINK_RE.match(line)), None)
        new_links = [f"[Unreleased]: {unreleased_url}", f"[{version}]: {version_url}"]
        if first_link is None:
            while kept and not kept[-1].strip():
                kept.pop()
            kept += [""] + new_links
        else:
            kept[first_link:first_link] = new_links
        lines = kept

    changelog.write_text("\n".join(lines).rstrip("\n") + "\n", encoding="utf-8")


#-------------------------------------------------------------------- selftest


def selftest() -> None:
    """Checks the bump rules and the file edits against throwaway copies."""

    cases = [
        ("0.1.0", "patch", None, "0.1.1"),
        ("0.1.3", "minor", None, "0.2.0"),
        ("0.4.1", "major", None, "1.0.0"),
        ("0.2.0-rc.1", "patch", None, "0.2.0"),
        ("0.2.0-rc.1", "minor", None, "0.2.0"),
        ("0.2.1-rc.1", "minor", None, "0.3.0"),
        ("1.0.0-beta.2", "major", None, "1.0.0"),
        ("0.1.0", "prerelease", None, "0.1.1-rc.1"),
        ("0.1.1-rc.1", "prerelease", None, "0.1.1-rc.2"),
        ("0.2.0-alpha.3", "prerelease", "beta", "0.2.0-beta.1"),
        ("0.1.3", "minor", "alpha", "0.2.0-alpha.1"),
        ("0.2.0-rc.2", "release", None, "0.2.0"),
    ]
    for start, part, pre, expected in cases:
        got = bump(start, part, pre)
        assert got == expected, f"{start} {part} {pre}: expected {expected}, got {got}"

    for start, part, pre in [("0.2.0", "release", None), ("0.2.0-rc.1", "prerelease", "alpha"), ("1.0", "patch", None)]:
        try:
            bump(start, part, pre)
        except VersionError:
            pass
        else:
            raise AssertionError(f"{start} {part} {pre} should have failed")

    with tempfile.TemporaryDirectory() as work:
        work = Path(work)
        manifest = work / "Cargo.toml"
        manifest.write_text(
            '[package]\nname = "demo"\nversion = "0.1.0"   # the version\nedition = "2024"\n\n'
            '[dependencies]\nserde = { version = "1" }\n\n[package.metadata.deb]\nversion = "ignored"\n',
            encoding="utf-8",
        )
        write_version(manifest, "0.2.0-rc.1")
        text = manifest.read_text(encoding="utf-8")
        assert 'version = "0.2.0-rc.1"   # the version' in text, text
        assert 'serde = { version = "1" }' in text and 'version = "ignored"' in text, text
        assert read_version(manifest) == "0.2.0-rc.1"

        changelog = work / "CHANGELOG.md"
        changelog.write_text(
            "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- A thing.\n\n"
            "## [0.1.0] - 2026-10-13\n\n- First.\n\n"
            "[Unreleased]: https://github.com/owner/repo/compare/v0.1.0...HEAD\n"
            "[0.1.0]: https://github.com/owner/repo/releases/tag/v0.1.0\n",
            encoding="utf-8",
        )
        release_changelog(changelog, "0.2.0", "2026-11-01")
        text = changelog.read_text(encoding="utf-8")
        assert "## [Unreleased]\n\n## [0.2.0] - 2026-11-01\n\n### Added\n\n- A thing.\n\n## [0.1.0]" in text, text
        assert "[Unreleased]: https://github.com/owner/repo/compare/v0.2.0...HEAD\n" in text, text
        assert "[0.2.0]: https://github.com/owner/repo/compare/v0.1.0...v0.2.0\n" in text, text
        assert "[0.1.0]: https://github.com/owner/repo/releases/tag/v0.1.0\n" in text, text
        assert notes(changelog, "0.2.0") == "### Added\n\n- A thing.\n", repr(notes(changelog, "0.2.0"))
        assert notes(changelog, "0.1.0") == "- First.\n", repr(notes(changelog, "0.1.0"))

        #Releasing an empty Unreleased section still makes a valid section.
        release_changelog(changelog, "0.2.1", "2026-11-02")
        assert "No user-facing changes." in notes(changelog, "0.2.1")
        try:
            release_changelog(changelog, "0.2.1", "2026-11-03")
        except VersionError:
            pass
        else:
            raise AssertionError("releasing the same version twice should fail")

    #The real files must parse, so a broken Cargo.toml or changelog fails CI here.
    real_version = read_version(ROOT / "Cargo.toml")
    real_lines = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8").splitlines()
    assert section_bounds(real_lines, "Unreleased") is not None, "CHANGELOG.md needs a '## [Unreleased]' section"
    print(f"version.py selftest passed (Cargo.toml is at {real_version})")


#------------------------------------------------------------------------ main


def main() -> int:
    parser = argparse.ArgumentParser(description="Read and change the version in Cargo.toml.")
    parser.add_argument("--manifest", type=Path, default=Path(os.environ.get("BRAND_MANIFEST", ROOT / "Cargo.toml")))
    parser.add_argument("--changelog", type=Path, default=ROOT / "CHANGELOG.md")
    commands = parser.add_subparsers(dest="command", required=True)

    commands.add_parser("current", help="print the version")

    bump_parser = commands.add_parser("bump", help="change the version")
    bump_parser.add_argument("part", choices=["patch", "minor", "major", "prerelease", "release"])
    bump_parser.add_argument("--pre", choices=PRE_LABELS, help="prerelease label")

    tag_parser = commands.add_parser("check-tag", help="check a tag matches the version")
    tag_parser.add_argument("tag")

    changelog_parser = commands.add_parser("changelog", help="release the Unreleased notes")
    changelog_parser.add_argument("version")
    changelog_parser.add_argument("--date", default=datetime.date.today().isoformat())

    notes_parser = commands.add_parser("notes", help="print one version's notes")
    notes_parser.add_argument("version")

    commands.add_parser("selftest", help="run the tests")

    args = parser.parse_args()
    try:
        if args.command == "current":
            print(read_version(args.manifest))
        elif args.command == "bump":
            new_version = bump(read_version(args.manifest), args.part, args.pre)
            write_version(args.manifest, new_version)
            print(new_version)
        elif args.command == "check-tag":
            version = read_version(args.manifest)
            if args.tag != f"v{version}":
                print(
                    f"tag {args.tag} does not match Cargo.toml version {version}; "
                    f"tag v{version} instead, or bump the version first",
                    file=sys.stderr,
                )
                return 1
            print(f"tag {args.tag} matches Cargo.toml")
        elif args.command == "changelog":
            release_changelog(args.changelog, args.version, args.date)
            print(f"CHANGELOG.md: Unreleased moved under {args.version}")
        elif args.command == "notes":
            sys.stdout.write(notes(args.changelog, args.version))
        elif args.command == "selftest":
            selftest()
    except VersionError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
