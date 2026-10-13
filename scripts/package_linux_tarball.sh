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
# Static libaom (video encoder) from the pinned, verified source: no libaom.so at runtime.
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
AOM_PREFIX="$(python3 scripts/buildlock.py --who linux-package -- python3 scripts/build_libaom.py --target host | tail -n1)"
export "AOM_DIR_${HOST_TRIPLE//-/_}=$AOM_PREFIX" AOM_STATIC_REQUIRED=1
python3 scripts/buildlock.py --who linux-package -- \
  cargo build --locked --release -p desktop --target-dir target/distribution
if readelf -d target/distribution/release/p2p-desktop | grep -q 'NEEDED.*libaom'; then
  echo "p2p-desktop links libaom dynamically; it must be static" >&2; exit 1
fi

OUT="dist/releases/v${VERSION}"
STAGE="target/package/tinline-${VERSION}-linux-${TARBALL_ARCH}"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/icons/hicolor/512x512/apps"
install -m 0755 target/distribution/release/p2p-desktop "$STAGE/bin/tinline"
install -m 0644 packaging/linux/tinline.desktop "$STAGE/share/applications/tinline.desktop"
install -m 0644 packaging/linux/tinline.png "$STAGE/share/icons/hicolor/512x512/apps/tinline.png"
install -m 0644 LICENSE "$STAGE/LICENSE"
install -Dm 0644 -t "$STAGE/licenses" packaging/licenses/*.txt
mkdir -p "$OUT"
tar -C "$(dirname "$STAGE")" -czf "$OUT/tinline-${VERSION}-linux-${TARBALL_ARCH}.tar.gz" "$(basename "$STAGE")"
sha256sum "$OUT/tinline-${VERSION}-linux-${TARBALL_ARCH}.tar.gz" > "$OUT/tinline-${VERSION}-linux-${TARBALL_ARCH}.tar.gz.sha256"
echo "$OUT/tinline-${VERSION}-linux-${TARBALL_ARCH}.tar.gz"
