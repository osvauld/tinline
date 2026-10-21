#!/usr/bin/env bash
# Builds p2pcore for iOS device + simulator, generates the Swift bindings, packs build/P2pCore.xcframework.
# macOS only (needs Xcode). Run from the repo root under the task manager: tasks run --peer mac -- ios/scripts/build_core.sh
set -euo pipefail
OUT=ios/build; GEN=$OUT/generated
rm -rf "$OUT/P2pCore.xcframework" "$GEN"; mkdir -p "$GEN"
for t in aarch64-apple-ios aarch64-apple-ios-sim; do
  rustup target add "$t"
  cargo build -j 6 -p p2pcore --release --lib --target "$t"
done
cargo run -p p2pcore --bin uniffi-bindgen -- generate \
  --library target/aarch64-apple-ios/release/libp2pcore.a --language swift --out-dir "$GEN"
mkdir -p "$GEN/headers" && cp "$GEN"/*.h "$GEN/headers/" && cp "$GEN"/*.modulemap "$GEN/headers/module.modulemap"
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libp2pcore.a -headers "$GEN/headers" \
  -library target/aarch64-apple-ios-sim/release/libp2pcore.a -headers "$GEN/headers" \
  -output "$OUT/P2pCore.xcframework"
echo "Swift sources to add to the app: $GEN/*.swift"
