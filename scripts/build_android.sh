#!/usr/bin/env bash
# Usage: scripts/build_android.sh [--install] [--abis x86_64[,arm64-v8a]] [--debug-rust]
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export PATH="$HOME/.cargo/bin:$ANDROID_HOME/platform-tools:$PATH"
if [ -d /usr/lib/jvm/java-21-openjdk ]; then export JAVA_HOME=/usr/lib/jvm/java-21-openjdk; fi
INSTALL=0; ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --install) INSTALL=1 ;;
    --abis) shift; ARGS+=("-Pabis=$1") ;;
    --debug-rust) ARGS+=("-PrustRelease=false") ;;
    *) echo "unknown arg $1" >&2; exit 2 ;;
  esac; shift
done
cd "$ROOT/android"
./gradlew --console=plain :app:assembleDebug ${ARGS[@]+"${ARGS[@]}"}
APK="$ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
echo "APK: $APK"
if [ "$INSTALL" = 1 ]; then
  adb install -r "$APK"
  adb shell am start -n com.osvauld.p2p/.MainActivity
fi
