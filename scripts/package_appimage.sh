#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
VERSION="${VERSION:-$(python3 - <<'PY'
import tomllib
with open('Cargo.toml', 'rb') as f:
    print(tomllib.load(f)['workspace']['package']['version'])
PY
)}"
# Native builds only: ARCH must describe both the executable and packaging tools.
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) TOOL_SHA256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d ;;
  aarch64) TOOL_SHA256=620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff ;;
  *) echo "unsupported AppImage arch: $ARCH" >&2; exit 1 ;;
esac
for tool in curl sha256sum readelf ldconfig; do
  command -v "$tool" >/dev/null || { echo "missing $tool" >&2; exit 1; }
done

# Pin and verify the packaging tool, rather than executing a mutable continuous release.
TOOL_VERSION=1-alpha-20251107-1
mkdir -p target/package
WORK="$(mktemp -d "$ROOT/target/package/appimage.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
TOOL="$WORK/linuxdeploy.AppImage"
curl --fail --location --retry 3 --output "$TOOL" \
  "https://github.com/linuxdeploy/linuxdeploy/releases/download/$TOOL_VERSION/linuxdeploy-$ARCH.AppImage"
printf '%s  %s\n' "$TOOL_SHA256" "$TOOL" | sha256sum --check
chmod +x "$TOOL"

# Keep distributable builds separate from development and test-hooks artifacts.
HOST_TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
AOM_PREFIX="$(python3 scripts/build_libaom.py --target host | tail -n1)"
# Static libvpx (desktop VP8, feature vp8) the same way: no libvpx.so at runtime.
VPX_PREFIX="$(python3 scripts/build_libvpx.py | tail -n1)"
export "AOM_DIR_${HOST_TRIPLE//-/_}=$AOM_PREFIX" AOM_STATIC_REQUIRED=1
export "VPX_DIR_${HOST_TRIPLE//-/_}=$VPX_PREFIX" VPX_STATIC_REQUIRED=1
cargo build --locked --release -p desktop --target-dir target/distribution
if readelf -d target/distribution/release/p2p-desktop | grep -q 'NEEDED.*libaom'; then
  echo 'p2p-desktop links libaom dynamically; it must be static' >&2; exit 1
fi
if readelf -d target/distribution/release/p2p-desktop | grep -q 'NEEDED.*libvpx'; then
  echo 'p2p-desktop links libvpx dynamically; it must be static' >&2; exit 1
fi

APPDIR="$WORK/Tinline.AppDir"
install -Dm 0755 target/distribution/release/p2p-desktop "$APPDIR/usr/bin/tinline"
install -Dm 0644 LICENSE "$APPDIR/usr/share/doc/tinline/copyright"
install -Dm 0644 -t "$APPDIR/usr/share/doc/tinline/third-party" packaging/licenses/*.txt

# Winit loads these at runtime, so an ELF dependency scan alone misses them.
# Graphics drivers/loaders remain host-provided, as do desktop portal/keyring services.
LIBRARIES=()
LIBRARY_MAP="$(ldconfig -p)"
for lib in libxkbcommon.so.0 libxkbcommon-x11.so.0 libwayland-client.so.0 \
           libwayland-cursor.so.0 libwayland-egl.so.1 libX11.so.6 libX11-xcb.so.1 \
           libXcursor.so.1 libXi.so.6 libXrandr.so.2; do
  path="$(awk -v lib="$lib" '$1 == lib {print $NF; exit}' <<< "$LIBRARY_MAP")"
  [[ -n "$path" ]] || { echo "missing runtime library: $lib" >&2; exit 1; }
  LIBRARIES+=(--library "$path")
done

OUT="$ROOT/dist/releases/v${VERSION}"
mkdir -p "$OUT"
# Extract-and-run works on CI and machines without FUSE. Run in our private workdir
# so linuxdeploy's temporary files cannot collide with another packaging session.
(
  cd "$WORK"
  # Rust release output is already stripped. linuxdeploy's older bundled strip
  # cannot parse RELR sections in libraries on newer build hosts.
  export ARCH VERSION APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1
  export OUTPUT="$WORK/tinline-${VERSION}-linux-${ARCH}.AppImage"
  python3 "$ROOT/scripts/buildlock.py" --who appimage-package --run -- \
    "$TOOL" --appdir "$APPDIR" \
    --desktop-file "$ROOT/packaging/linux/tinline.desktop" \
    --icon-file "$ROOT/packaging/linux/tinline.png" \
    "${LIBRARIES[@]}" --output appimage
)
IMAGE="tinline-${VERSION}-linux-${ARCH}.AppImage"
install -m 0755 "$WORK/$IMAGE" "$OUT/$IMAGE"
(cd "$OUT" && sha256sum "$IMAGE" > "$IMAGE.sha256")
echo "$OUT/$IMAGE"
