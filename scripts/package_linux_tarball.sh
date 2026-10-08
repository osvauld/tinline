#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${VERSION:-$(python3 - <<'PY'
import tomllib
with open('Cargo.toml','rb') as f:
    print(tomllib.load(f)['workspace']['package']['version'])
PY
)}"
ARCH="${ARCH:-$(uname -m)}"
case "$ARCH" in
  x86_64|amd64) DEB_ARCH=amd64; TARBALL_ARCH=x86_64 ;;
  aarch64|arm64) DEB_ARCH=arm64; TARBALL_ARCH=aarch64 ;;
  *) echo "unsupported arch: $ARCH" >&2; exit 1 ;;
esac

# Isolate distributable builds from development/test-hooks artifacts.
python3 scripts/buildlock.py --who linux-package -- \
  cargo build --locked --release -p desktop --target-dir target/distribution

OUT="dist/releases/v${VERSION}"
STAGE="target/package/tinline-${VERSION}-${TARBALL_ARCH}-linux"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/icons/hicolor/512x512/apps"
install -m 0755 target/distribution/release/p2p-desktop "$STAGE/bin/tinline"
install -m 0644 packaging/linux/tinline.desktop "$STAGE/share/applications/tinline.desktop"
install -m 0644 packaging/linux/tinline.png "$STAGE/share/icons/hicolor/512x512/apps/tinline.png"
install -m 0644 LICENSE "$STAGE/LICENSE"
mkdir -p "$OUT"
tar -C "$(dirname "$STAGE")" -czf "$OUT/tinline-v${VERSION}-${TARBALL_ARCH}-linux.tar.gz" "$(basename "$STAGE")"
sha256sum "$OUT/tinline-v${VERSION}-${TARBALL_ARCH}-linux.tar.gz" > "$OUT/tinline-v${VERSION}-${TARBALL_ARCH}-linux.tar.gz.sha256"
echo "$OUT/tinline-v${VERSION}-${TARBALL_ARCH}-linux.tar.gz"
