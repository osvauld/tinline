#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
REPO="${REPO:-dist/repo/apt}"
ARCH="${ARCH:-amd64}"
: "${GPG_KEY_ID:?set the full APT signing key fingerprint}"
for tool in dpkg-scanpackages apt-ftparchive gpg; do
  command -v "$tool" >/dev/null || { echo "missing $tool" >&2; exit 1; }
done
mkdir -p "$REPO/pool/main/t/tinline" "$REPO/dists/stable/main/binary-$ARCH"
cp dist/deb/tinline_*_${ARCH}.deb "$REPO/pool/main/t/tinline/"
pushd "$REPO" >/dev/null
INDEX="dists/stable/main/binary-$ARCH"
dpkg-scanpackages --multiversion --arch "$ARCH" pool > "$INDEX/Packages"
gzip -9nc "$INDEX/Packages" > "$INDEX/Packages.gz"
# Generate before adding by-hash aliases; never checksum old signatures.
rm -f dists/stable/Release dists/stable/Release.gpg dists/stable/InRelease
apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=Osvauld \
  -o APT::FTPArchive::Release::Label=Tinline \
  -o APT::FTPArchive::Release::Suite=stable \
  -o APT::FTPArchive::Release::Codename=stable \
  -o APT::FTPArchive::Release::Architectures="$ARCH" \
  -o APT::FTPArchive::Release::Components=main \
  -o APT::FTPArchive::Release::Acquire-By-Hash=yes \
  -o APT::FTPArchive::Release::Valid-Until="$(date -u -d '+30 days' -R)" \
  release dists/stable > dists/stable/Release
mkdir -p "$INDEX/by-hash/SHA256" "$INDEX/by-hash/SHA512"
for file in "$INDEX/Packages" "$INDEX/Packages.gz"; do
  cp "$file" "$INDEX/by-hash/SHA256/$(sha256sum "$file" | cut -d' ' -f1)"
  cp "$file" "$INDEX/by-hash/SHA512/$(sha512sum "$file" | cut -d' ' -f1)"
done
SIGN_ARGS=(--batch --yes --local-user "$GPG_KEY_ID")
if [[ -n "${GPG_PASSPHRASE_FILE:-}" ]]; then
  SIGN_ARGS+=(--pinentry-mode loopback --passphrase-file "$GPG_PASSPHRASE_FILE")
fi
gpg "${SIGN_ARGS[@]}" --detach-sign --armor -o dists/stable/Release.gpg dists/stable/Release
gpg "${SIGN_ARGS[@]}" --clearsign -o dists/stable/InRelease dists/stable/Release
gpg --verify dists/stable/InRelease
gpg --batch --yes --export "$GPG_KEY_ID" > ../tinline.gpg
popd >/dev/null
