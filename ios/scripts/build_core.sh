#!/usr/bin/env bash
# Builds p2pcore for iOS device + simulator, generates the Swift bindings, packs build/P2pCore.xcframework.
# macOS only (needs Xcode, cmake, ninja). Run from the repo root: ios/scripts/build_core.sh
set -euo pipefail
OUT=ios/build; GEN=$OUT/generated
# Same minimum for rustc and every cc build (ring, opus, ...); rustc's default 10.0 can't link opus's
# ___chkstk_darwin. Keep in step with ios/project.yml deploymentTarget. Set only for the iOS builds:
# Apple's clang reads it and would build the host libaom below for iOS too.
IOS_MIN=17.0
rm -rf "$OUT/P2pCore.xcframework" "$GEN"; mkdir -p "$GEN"
for t in aarch64-apple-ios aarch64-apple-ios-sim; do
  rustup target add "$t"
  # Static AV1 encoder for this target; the video crate links it via AOM_DIR_<triple>.
  aom=$(IPHONEOS_DEPLOYMENT_TARGET=$IOS_MIN python3 scripts/build_libaom.py --target "$t" | tail -n 1)
  env IPHONEOS_DEPLOYMENT_TARGET=$IOS_MIN "AOM_DIR_${t//-/_}=$aom" cargo build -j 6 -p p2pcore --release --lib --target "$t"
done
# uniffi-bindgen is a host binary that links p2pcore (and so libaom) for macOS too.
host_aom=$(python3 scripts/build_libaom.py --target host | tail -n 1)
AOM_DIR="$host_aom" cargo run -j 6 -p p2pcore --bin uniffi-bindgen -- generate \
  --library target/aarch64-apple-ios/release/libp2pcore.a --language swift --out-dir "$GEN"
# One module map for both generated modules (p2pcoreFFI, presentFFI).
mkdir -p "$GEN/headers" && cp "$GEN"/*.h "$GEN/headers/" && cat "$GEN"/*.modulemap > "$GEN/headers/module.modulemap"
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libp2pcore.a -headers "$GEN/headers" \
  -library target/aarch64-apple-ios-sim/release/libp2pcore.a -headers "$GEN/headers" \
  -output "$OUT/P2pCore.xcframework"
echo "Swift sources to add to the app: $GEN/*.swift"
