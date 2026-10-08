#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CODENAME="${CODENAME:-stable}"
COMPONENT="${COMPONENT:-main}"
ARCH="${ARCH:-amd64}"
REPO="${REPO:-dist/repo/apt}"
GPG_KEY_ID="${GPG_KEY_ID:-}"

command -v dpkg-scanpackages >/dev/null || { echo "missing dpkg-scanpackages; install dpkg-dev" >&2; exit 1; }
command -v apt-ftparchive >/dev/null || { echo "missing apt-ftparchive; install apt-utils" >&2; exit 1; }

mkdir -p "$REPO/pool/main/t/tinline" "$REPO/dists/$CODENAME/$COMPONENT/binary-$ARCH"
cp dist/deb/tinline_*_${ARCH}.deb "$REPO/pool/main/t/tinline/"

pushd "$REPO" >/dev/null
dpkg-scanpackages --arch "$ARCH" pool > "dists/$CODENAME/$COMPONENT/binary-$ARCH/Packages"
gzip -9c "dists/$CODENAME/$COMPONENT/binary-$ARCH/Packages" > "dists/$CODENAME/$COMPONENT/binary-$ARCH/Packages.gz"
apt-ftparchive release "dists/$CODENAME" > "dists/$CODENAME/Release"
if [[ -n "$GPG_KEY_ID" ]]; then
  gpg --batch --yes --local-user "$GPG_KEY_ID" --detach-sign --armor -o "dists/$CODENAME/Release.gpg" "dists/$CODENAME/Release"
  gpg --batch --yes --local-user "$GPG_KEY_ID" --clearsign -o "dists/$CODENAME/InRelease" "dists/$CODENAME/Release"
else
  echo "GPG_KEY_ID not set; repo metadata is unsigned. Set GPG_KEY_ID for releases." >&2
fi
popd >/dev/null

echo "$REPO"
