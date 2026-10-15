#!/usr/bin/env bash
#Vmynt
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)

#Builds a signed apt repository from a folder of .deb files.
#
#Usage:
#  packaging/apt/build-repo.sh DEBS_DIR OUT_DIR
#
#OUT_DIR is wiped and rebuilt every time, so the repository is always exactly
#"the .deb files you gave it", never a pile of leftovers. The result is plain
#static files, which is why GitHub Pages can host it:
#
#  OUT_DIR/
#    pool/main/<letter>/<package>/<file>.deb    the packages themselves
#    dists/stable/main/binary-amd64/Packages    the index apt downloads (and .gz)
#    dists/stable/main/binary-arm64/Packages
#    dists/stable/Release                       lists the indexes and their hashes
#    dists/stable/InRelease                     Release with the signature inline
#    dists/stable/Release.gpg                   the signature on its own (older apt)
#    <name>-archive-keyring.gpg                 public key, for "signed-by="
#    <name>-archive-keyring.asc                 the same key as text
#    index.html                                 how to add the repository
#
#Signing uses a GPG secret key that is already imported into the keyring
#(the pages workflow imports it from a secret). Settings, all optional:
#
#  APT_GPG_KEY_ID       which key to sign with; default: the first secret key
#  APT_GPG_PASSPHRASE   the key's passphrase, if it has one
#  APT_SUITE            default "stable"
#  APT_COMPONENT        default "main"
#  APT_ARCHES           default "amd64 arm64"
#  APT_URL              public url of OUT_DIR, for index.html; default from Cargo.toml
#
#Needs bash, gpg, dpkg-deb and python3 (3.11+). apt-utils is not needed.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGING="$(dirname "$HERE")"

if [ "$#" -ne 2 ]; then
    echo "usage: $0 DEBS_DIR OUT_DIR" >&2
    exit 2
fi

DEBS_DIR="$1"
OUT_DIR="$2"
SUITE="${APT_SUITE:-stable}"
COMPONENT="${APT_COMPONENT:-main}"
ARCHES="${APT_ARCHES:-amd64 arm64}"

for tool in gpg gpgv dpkg-deb python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "build-repo.sh: $tool is not installed" >&2
        exit 1
    fi
done

#The product name and display name come from Cargo.toml, never typed here.
NAME="$(python3 "$PACKAGING/brand.py" --get name)"
DISPLAY_NAME="$(python3 "$PACKAGING/brand.py" --get display_name)"
KEYRING="$(python3 "$PACKAGING/brand.py" --get apt_keyring)"
APT_URL="${APT_URL:-$(python3 "$PACKAGING/brand.py" --get apt_url)}"
APT_URL="${APT_URL%/}"

#---- pick the signing key ---------------------------------------------------

KEY_ID="${APT_GPG_KEY_ID:-}"
if [ -z "$KEY_ID" ]; then
    #--with-colons is gpg's machine readable listing; field 10 of an "fpr" line
    #is a fingerprint, and the first one belongs to the first secret key.
    KEY_ID="$(gpg --batch --list-secret-keys --with-colons 2>/dev/null | awk -F: '$1 == "fpr" { print $10; exit }')"
fi
if [ -z "$KEY_ID" ]; then
    echo "build-repo.sh: no GPG secret key to sign with (import one, or set APT_GPG_KEY_ID)" >&2
    exit 3
fi

#gpg options shared by both signatures. A passphrase is handed over on file
#descriptor 3, so it never shows up in the process list.
SIGN=(gpg --batch --yes --local-user "$KEY_ID")
if [ -n "${APT_GPG_PASSPHRASE:-}" ]; then
    SIGN+=(--pinentry-mode loopback --passphrase-fd 3)
fi

sign() {
    if [ -n "${APT_GPG_PASSPHRASE:-}" ]; then
        "${SIGN[@]}" "$@" 3<<<"$APT_GPG_PASSPHRASE"
    else
        "${SIGN[@]}" "$@"
    fi
}

#---- copy the packages into the pool ----------------------------------------

shopt -s nullglob
debs=("$DEBS_DIR"/*.deb)
if [ "${#debs[@]}" -eq 0 ]; then
    echo "build-repo.sh: no .deb files in $DEBS_DIR" >&2
    exit 4
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

for deb in "${debs[@]}"; do
    package="$(dpkg-deb --field "$deb" Package)"
    #Debian's pool layout: pool/<component>/<first letter>/<package>/
    target="$OUT_DIR/pool/$COMPONENT/${package:0:1}/$package"
    mkdir -p "$target"
    cp "$deb" "$target/"
    echo "added $(basename "$deb")"
done

#---- write the indexes ------------------------------------------------------

arch_args=()
for arch in $ARCHES; do
    arch_args+=(--arch "$arch")
done

python3 "$HERE/make_index.py" "$OUT_DIR" \
    --suite "$SUITE" \
    --component "$COMPONENT" \
    "${arch_args[@]}" \
    --origin "$DISPLAY_NAME" \
    --label "$DISPLAY_NAME" \
    --description "$DISPLAY_NAME packages"

#---- sign -------------------------------------------------------------------

RELEASE="$OUT_DIR/dists/$SUITE/Release"
sign --clearsign --output "$OUT_DIR/dists/$SUITE/InRelease" "$RELEASE"
sign --armor --detach-sign --output "$OUT_DIR/dists/$SUITE/Release.gpg" "$RELEASE"

#apt's "signed-by=" wants the binary form; the .asc is for people who want to read it.
gpg --batch --export "$KEY_ID" > "$OUT_DIR/$KEYRING.gpg"
gpg --batch --armor --export "$KEY_ID" > "$OUT_DIR/$KEYRING.asc"

#Check the result the way apt will: gpgv against only the published public key.
gpgv --keyring "$OUT_DIR/$KEYRING.gpg" "$OUT_DIR/dists/$SUITE/InRelease" >/dev/null 2>&1 || {
    echo "build-repo.sh: InRelease does not verify against $KEYRING.gpg" >&2
    exit 5
}
gpgv --keyring "$OUT_DIR/$KEYRING.gpg" "$OUT_DIR/dists/$SUITE/Release.gpg" "$RELEASE" >/dev/null 2>&1 || {
    echo "build-repo.sh: Release.gpg does not verify against $KEYRING.gpg" >&2
    exit 5
}

#---- a page for anyone who opens the url in a browser -----------------------

cat > "$OUT_DIR/index.html" <<EOF
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<meta name="robots" content="noindex" />
<title>$DISPLAY_NAME apt repository</title>
<style>
body { font-family: system-ui, sans-serif; max-width: 46rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.6; background: #0e0f13; color: #f3f4f6; }
pre { background: #0b0c10; border: 1px solid #2b2d38; padding: 1rem; border-radius: 0.5rem; overflow-x: auto; }
a { color: #3ddc97; }
</style>
</head>
<body>
<h1>$DISPLAY_NAME apt repository</h1>
<p>For Debian, Ubuntu and other apt based systems, on amd64 and arm64.</p>
<pre>curl -fsSL $APT_URL/$KEYRING.gpg | sudo tee /usr/share/keyrings/$KEYRING.gpg &gt;/dev/null
echo "deb [signed-by=/usr/share/keyrings/$KEYRING.gpg] $APT_URL $SUITE $COMPONENT" | sudo tee /etc/apt/sources.list.d/$NAME.list
sudo apt update &amp;&amp; sudo apt install $NAME</pre>
<p>Signing key: <a href="./$KEYRING.asc">$KEYRING.asc</a> (fingerprint <code>$KEY_ID</code>)</p>
</body>
</html>
EOF

echo "apt repository ready in $OUT_DIR (signed with $KEY_ID)"
