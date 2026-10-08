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
ARCH="${ARCH:-$(dpkg --print-architecture 2>/dev/null || uname -m)}"
case "$ARCH" in
  x86_64|amd64) DEB_ARCH=amd64 ;;
  aarch64|arm64) DEB_ARCH=arm64 ;;
  *) echo "unsupported arch: $ARCH" >&2; exit 1 ;;
esac

for tool in dpkg-deb dpkg-shlibdeps; do
  command -v "$tool" >/dev/null || { echo "missing $tool; build on Debian/Ubuntu with dpkg-dev installed" >&2; exit 1; }
done

python3 scripts/buildlock.py --who deb-package -- \
  cargo build --locked --release -p desktop --target-dir target/distribution

PKGROOT="target/package/deb/tinline_${VERSION}_${DEB_ARCH}"
rm -rf "$PKGROOT"
mkdir -p "$PKGROOT/DEBIAN" \
  "$PKGROOT/usr/bin" \
  "$PKGROOT/usr/share/applications" \
  "$PKGROOT/usr/share/icons/hicolor/512x512/apps" \
  "$PKGROOT/usr/share/doc/tinline"

install -m 0755 target/distribution/release/p2p-desktop "$PKGROOT/usr/bin/tinline"
install -m 0644 packaging/linux/tinline.desktop "$PKGROOT/usr/share/applications/tinline.desktop"
install -m 0644 packaging/linux/tinline.png "$PKGROOT/usr/share/icons/hicolor/512x512/apps/tinline.png"
install -m 0644 LICENSE "$PKGROOT/usr/share/doc/tinline/copyright"

# Resolve versioned runtime dependencies against the build distro, not a guessed list.
DEPS_DIR="$(mktemp -d)"
trap 'rm -rf "$DEPS_DIR"' EXIT
mkdir -p "$DEPS_DIR/debian"
printf 'Source: tinline\n\nPackage: tinline\nArchitecture: any\n' > "$DEPS_DIR/debian/control"
DEPENDS="$(cd "$DEPS_DIR" && dpkg-shlibdeps -O -e"$ROOT/$PKGROOT/usr/bin/tinline")"
DEPENDS="${DEPENDS#shlibs:Depends=}"
INSTALLED_SIZE="$(du -sk "$PKGROOT/usr" | cut -f1)"
cat > "$PKGROOT/DEBIAN/control" <<CONTROL
Package: tinline
Version: ${VERSION}
Section: net
Priority: optional
Architecture: ${DEB_ARCH}
Maintainer: Osvauld <contact@osvauld.com>
Installed-Size: ${INSTALLED_SIZE}
Depends: ${DEPENDS}, libsecret-1-0, xdg-desktop-portal
Recommends: gnome-keyring | kwalletmanager
Homepage: https://tinline.osvauld.com
Description: Peer-to-peer voice calls and 1:1 chat
 Tinline is an Osvauld product for peer-to-peer voice calls and 1:1 chat.
CONTROL

mkdir -p dist/deb
dpkg-deb --build --root-owner-group "$PKGROOT" "dist/deb/tinline_${VERSION}_${DEB_ARCH}.deb"
sha256sum "dist/deb/tinline_${VERSION}_${DEB_ARCH}.deb" > "dist/deb/tinline_${VERSION}_${DEB_ARCH}.deb.sha256"
echo "dist/deb/tinline_${VERSION}_${DEB_ARCH}.deb"
