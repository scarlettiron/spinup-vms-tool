#!/usr/bin/env bash
#Vmseed
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)

#Tests the packaging scripts without publishing anything.
#
#  1. Renders every template (Homebrew, Chocolatey, winget) against a made up
#     SHA256SUMS, and checks nothing was left unfilled and each file parses.
#  2. Builds two throwaway .deb files and a throwaway GPG key, builds the apt
#     repository from them, and checks the indexes and the signatures.
#  3. If apt-get is installed, points a private apt-get at the repository (no
#     root, nothing touches the system's apt) and checks it accepts it.
#
#Usage: packaging/test.sh            (KEEP_WORK=1 keeps the files it builds)
#CI runs this on every pull request. Set BRAND_MANIFEST to test against a
#Cargo.toml other than the one in the repository root.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d)"
#KEEP_WORK=1 leaves the temporary folder behind, to look at what was built.
if [ "${KEEP_WORK:-}" = "1" ]; then
    echo "working in $WORK (kept)"
else
    trap 'rm -rf "$WORK"' EXIT
fi

pass() { echo "  ok  $*"; }
fail() { echo "FAIL  $*" >&2; exit 1; }

brand() { python3 "$HERE/brand.py" --get "$1"; }

NAME="$(brand name)"
VERSION="$(brand version)"
echo "testing packaging for $NAME $VERSION"

#---- 1. templates -----------------------------------------------------------

echo "templates:"

#A SHA256SUMS listing every file a real release has. The hashes are made up.
SUMS="$WORK/SHA256SUMS"
: > "$SUMS"
for target in x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do
    echo "$(echo "$target" | sha256sum | cut -d' ' -f1)  $NAME-$VERSION-$target.zip" >> "$SUMS"
done
for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin; do
    echo "$(echo "$target" | sha256sum | cut -d' ' -f1)  $NAME-$VERSION-$target.tar.gz" >> "$SUMS"
done
for arch in amd64 arm64; do
    echo "$(echo "$arch" | sha256sum | cut -d' ' -f1)  ${NAME}_${VERSION}_$arch.deb" >> "$SUMS"
done
for arch in x86_64 aarch64; do
    echo "$(echo "$arch" | sha256sum | cut -d' ' -f1)  $NAME-$VERSION-1.$arch.rpm" >> "$SUMS"
done

OUT="$WORK/rendered"
python3 "$HERE/render.py" --sums "$SUMS" \
    "$HERE/homebrew/formula.rb.tmpl=$OUT/homebrew/Formula/{{name}}.rb" \
    "$HERE/chocolatey/package.nuspec.tmpl=$OUT/choco/package.nuspec" \
    "$HERE/chocolatey/tools/chocolateyinstall.ps1.tmpl=$OUT/choco/tools/chocolateyinstall.ps1" \
    "$HERE/winget/version.yaml.tmpl=$OUT/winget/{{winget_id}}.yaml" \
    "$HERE/winget/installer.yaml.tmpl=$OUT/winget/{{winget_id}}.installer.yaml" \
    "$HERE/winget/locale.en-US.yaml.tmpl=$OUT/winget/{{winget_id}}.locale.en-US.yaml" \
    > /dev/null

if grep -rn '{{' "$OUT"; then
    fail "a rendered file still has a {{placeholder}}"
fi
pass "every placeholder filled"

#A placeholder with no value must stop the render, not produce an empty url.
echo 'url "{{url_no_such_target}}"' > "$WORK/bad.tmpl"
if python3 "$HERE/render.py" --sums "$SUMS" "$WORK/bad.tmpl=$WORK/bad.out" > /dev/null 2>&1; then
    fail "render.py accepted a placeholder with no value"
fi
pass "unknown placeholder is an error"

python3 - "$OUT" <<'PY'
import sys, xml.dom.minidom
from pathlib import Path
import yaml

out = Path(sys.argv[1])
xml.dom.minidom.parse(str(out / "choco" / "package.nuspec"))
for manifest in (out / "winget").glob("*.yaml"):
    data = yaml.safe_load(manifest.read_text())
    assert data["PackageIdentifier"], manifest
    assert "ManifestType" in data, manifest
installer = yaml.safe_load(next((out / "winget").glob("*.installer.yaml")).read_text())
for entry in installer["Installers"]:
    assert entry["InstallerUrl"].startswith("https://github.com/"), entry
    assert len(entry["InstallerSha256"]) == 64, entry
PY
pass "nuspec is valid XML, winget manifests are valid YAML"

if command -v ruby >/dev/null 2>&1; then
    ruby -c "$OUT/homebrew/Formula/$NAME.rb" > /dev/null || fail "formula is not valid Ruby"
    pass "formula is valid Ruby"
else
    echo "  --  ruby not installed, formula syntax not checked"
fi

#---- 2. apt repository ------------------------------------------------------

echo "apt repository:"

#Two tiny packages, one per architecture, with the real naming scheme.
DEBS="$WORK/debs"
mkdir -p "$DEBS"
for arch in amd64 arm64; do
    root="$WORK/pkg-$arch"
    mkdir -p "$root/DEBIAN" "$root/usr/bin"
    printf '#!/bin/sh\necho %s %s\n' "$NAME" "$VERSION" > "$root/usr/bin/$NAME"
    chmod 755 "$root/usr/bin/$NAME"
    cat > "$root/DEBIAN/control" <<EOF
Package: $NAME
Version: $VERSION
Architecture: $arch
Maintainer: Packaging Test <test@example.invalid>
Section: utils
Priority: optional
Description: packaging test build
 Not a real package.
EOF
    dpkg-deb --root-owner-group --build "$root" "$DEBS/${NAME}_${VERSION}_$arch.deb" > /dev/null
done
pass "built two .deb files"

#A throwaway key, with a passphrase so that path gets tested too.
export GNUPGHOME="$WORK/gnupg"
mkdir -m 700 "$GNUPGHOME"
gpg --batch --pinentry-mode loopback --passphrase test-passphrase \
    --quick-gen-key "Packaging Test <test@example.invalid>" rsa2048 sign never 2> /dev/null
pass "made a throwaway signing key"

REPO="$WORK/apt"
APT_GPG_PASSPHRASE=test-passphrase APT_URL="file://$REPO" \
    bash "$HERE/apt/build-repo.sh" "$DEBS" "$REPO" > "$WORK/build-repo.log" \
    || { cat "$WORK/build-repo.log"; fail "build-repo.sh failed"; }
pass "build-repo.sh ran (and checked its own signatures with gpgv)"

python3 - "$REPO" "$NAME" "$VERSION" <<'PY'
import gzip, hashlib, sys
from pathlib import Path

repo, name, version = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
dist = repo / "dists" / "stable"

for arch in ("amd64", "arm64"):
    folder = dist / "main" / f"binary-{arch}"
    text = (folder / "Packages").read_text()
    assert gzip.decompress((folder / "Packages.gz").read_bytes()).decode() == text, "Packages.gz differs"
    fields = dict(line.split(": ", 1) for line in text.splitlines() if ": " in line and not line.startswith(" "))
    assert fields["Package"] == name and fields["Version"] == version and fields["Architecture"] == arch, fields
    deb = repo / fields["Filename"]
    data = deb.read_bytes()
    assert int(fields["Size"]) == len(data), "Size is wrong"
    assert fields["SHA256"] == hashlib.sha256(data).hexdigest(), "SHA256 is wrong"

#Every file Release lists must have the size and hash Release says it has.
section = None
checked = 0
for line in (dist / "Release").read_text().splitlines():
    if not line.startswith(" "):
        section = line.rstrip(":")
        continue
    if section != "SHA256":
        continue
    digest, size, path = line.split()
    data = (dist / path).read_bytes()
    assert len(data) == int(size) and hashlib.sha256(data).hexdigest() == digest, path
    checked += 1
assert checked == 4, f"expected 4 index files in Release, found {checked}"
PY
pass "Packages and Release have the right fields, sizes and hashes"

KEYRING="$(brand apt_keyring)"
if [ ! -s "$REPO/$KEYRING.gpg" ] || [ ! -s "$REPO/$KEYRING.asc" ]; then
    fail "public key files missing"
fi
pass "public keyring exported"

#---- 3. a real apt-get, if there is one -------------------------------------

if command -v apt-get >/dev/null 2>&1; then
    APT="$WORK/apt-client"
    mkdir -p "$APT/state/lists/partial" "$APT/cache/archives/partial" "$APT/etc/trusted.gpg.d" "$APT/etc/preferences.d"
    : > "$APT/state/status"
    #Only the architecture the test machine has, so apt does not ask for the other.
    echo "deb [signed-by=$REPO/$KEYRING.gpg] file://$REPO stable main" > "$APT/sources.list"
    apt_opts=(
        -o "Dir::State=$APT/state" -o "Dir::State::status=$APT/state/status"
        -o "Dir::Cache=$APT/cache" -o "Dir::Etc::SourceList=$APT/sources.list"
        -o "Dir::Etc::SourceParts=/nonexistent" -o "Dir::Etc::TrustedParts=$APT/etc/trusted.gpg.d"
        -o "Dir::Etc::Preferences=/nonexistent" -o "Dir::Etc::PreferencesParts=$APT/etc/preferences.d"
        -o "Debug::NoLocking=1" -o "APT::Sandbox::User=$(id -un)"
    )
    apt-get "${apt_opts[@]}" update > "$WORK/apt-update.log" 2>&1 \
        || { cat "$WORK/apt-update.log"; fail "apt-get update rejected the repository"; }
    if grep -qiE '^(W|E):' "$WORK/apt-update.log"; then
        cat "$WORK/apt-update.log"
        fail "apt-get update warned about the repository"
    fi
    apt-cache "${apt_opts[@]}" policy "$NAME" | grep -q "$VERSION" \
        || fail "apt-cache does not see $NAME $VERSION"
    pass "apt-get update accepts the signed repository and sees $NAME $VERSION"
else
    echo "  --  apt-get not installed, skipped the real apt check"
fi

echo "all packaging tests passed"
