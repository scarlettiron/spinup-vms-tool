#!/usr/bin/env python3
#Vmseed
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)

"""Writes the index files of a small apt repository.

apt does not read the .deb files directly. It reads two kinds of index:

    dists/<suite>/<component>/binary-<arch>/Packages   one entry per .deb: its
                                                       control fields, where it
                                                       is, its size and hashes
    dists/<suite>/Release                              the list of Packages files
                                                       with their sizes and hashes

build-repo.sh signs Release afterwards, which is what makes apt trust the whole
thing: the signature covers Release, Release covers each Packages file, and each
Packages file covers each .deb.

Debian's own tool for this is apt-ftparchive. This script does the same job with
only Python and dpkg-deb, so the repository builds the same way on a laptop
without apt-utils as it does in CI.

Usage:
    make_index.py REPO_DIR --suite stable --component main --arch amd64 --arch arm64 \
                  --origin NAME --label NAME --description TEXT
REPO_DIR must already contain pool/<component>/.../*.deb.
"""

import argparse
import email.utils
import gzip
import hashlib
import subprocess
from pathlib import Path

#The hashes apt checks, in the order they appear in each entry.
HASHES = (("MD5sum", "md5"), ("SHA1", "sha1"), ("SHA256", "sha256"))
RELEASE_HASHES = (("MD5Sum", "md5"), ("SHA1", "sha1"), ("SHA256", "sha256"))


def digest(data: bytes, algorithm: str) -> str:
    return hashlib.new(algorithm, data).hexdigest()


def control_fields(deb: Path) -> str:
    """The package's control paragraph, exactly as dpkg-deb prints it."""

    result = subprocess.run(["dpkg-deb", "--field", str(deb)], check=True, capture_output=True, text=True)
    return result.stdout.rstrip("\n")


def field(paragraph: str, name: str) -> str:
    for line in paragraph.splitlines():
        if line.lower().startswith(f"{name.lower()}:"):
            return line.split(":", 1)[1].strip()
    raise SystemExit(f"control file has no {name} field:\n{paragraph}")


def version_key(version: str) -> list:
    """A rough sort key so older versions are listed first. apt sorts properly
    itself; this only keeps the file tidy and stable between runs."""

    return [int(part) if part.isdigit() else part for part in version.replace("-", ".").replace("~", ".").split(".")]


def packages_entries(repo: Path, component: str) -> list[tuple[str, str, str, str]]:
    """One (name, version, architecture, entry text) per .deb in the pool."""

    entries = []
    for deb in sorted((repo / "pool" / component).rglob("*.deb")):
        data = deb.read_bytes()
        paragraph = control_fields(deb)

        lines = [paragraph]
        lines.append(f"Filename: {deb.relative_to(repo).as_posix()}")
        lines.append(f"Size: {len(data)}")
        for label, algorithm in HASHES:
            lines.append(f"{label}: {digest(data, algorithm)}")

        entries.append(
            (field(paragraph, "Package"), field(paragraph, "Version"), field(paragraph, "Architecture"), "\n".join(lines))
        )

    entries.sort(key=lambda entry: (entry[0], version_key(entry[1])))
    return entries


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("repo", type=Path)
    parser.add_argument("--suite", default="stable")
    parser.add_argument("--component", default="main")
    parser.add_argument("--arch", action="append", required=True, help="repeat for each architecture")
    parser.add_argument("--origin", required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--description", default="")
    args = parser.parse_args()

    repo: Path = args.repo
    dist = repo / "dists" / args.suite
    entries = packages_entries(repo, args.component)

    #Every file listed in Release, as a path relative to dists/<suite>/.
    index_files: list[Path] = []

    for arch in args.arch:
        folder = dist / args.component / f"binary-{arch}"
        folder.mkdir(parents=True, exist_ok=True)

        #A package built for "all" architectures belongs in every list.
        chosen = [entry[3] for entry in entries if entry[2] in (arch, "all")]
        text = ("\n\n".join(chosen) + "\n") if chosen else ""

        packages = folder / "Packages"
        packages.write_text(text, encoding="utf-8")
        #mtime=0 keeps the gzip identical between runs when the content is identical.
        with packages.with_suffix(".gz").open("wb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
                compressed.write(text.encode("utf-8"))

        index_files += [packages, packages.with_suffix(".gz")]
        print(f"binary-{arch}: {len(chosen)} package(s)")

    release = [
        f"Origin: {args.origin}",
        f"Label: {args.label}",
        f"Suite: {args.suite}",
        f"Codename: {args.suite}",
        f"Date: {email.utils.formatdate(usegmt=True)}",
        f"Architectures: {' '.join(args.arch)}",
        f"Components: {args.component}",
    ]
    if args.description:
        release.append(f"Description: {args.description}")

    for label, algorithm in RELEASE_HASHES:
        release.append(f"{label}:")
        for path in index_files:
            data = path.read_bytes()
            release.append(f" {digest(data, algorithm)} {len(data):>8} {path.relative_to(dist).as_posix()}")

    (dist / "Release").write_text("\n".join(release) + "\n", encoding="utf-8")
    print(f"wrote {dist / 'Release'}")


if __name__ == "__main__":
    main()
