# Building Tinline on a Mac

Two things to try, in this order. Neither has ever been built on a Mac, so the first run will
likely stop on compile or link errors. That is expected: the useful result is the **full log**.

Branch: `feature/ios-app`. Run everything from the repo root.

## 0. One-time setup

```sh
xcode-select --install            # or full Xcode from the App Store (needed for part 2)
brew install cmake ninja meson pkg-config xcodegen python3
curl https://sh.rustup.rs -sSf | sh   # rustup; rust-toolchain.toml picks the Rust version itself
```

## 1. Desktop app (iced, macOS native)

```sh
aom=$(python3 scripts/build_libaom.py --target host | tail -n 1)   # static AV1 encoder, ~5 min
AOM_DIR="$aom" cargo build --release -p desktop 2>&1 | tee desktop-build.log
./target/release/p2p-desktop
```

What to expect if it runs: chat, groups and voice calls work; video can be **received** but not
sent (no macOS camera capture yet); no menu-bar icon (closing the window minimises it). On first
call macOS asks for microphone access for your terminal app; allow it.

## 2. iOS app (SwiftUI, simulator)

```sh
ios/scripts/build_core.sh 2>&1 | tee ios-core.log            # Rust core for iPhone + simulator, ~20 min
xcodegen generate --spec ios/project.yml
xcodebuild -project ios/Tinline.xcodeproj -scheme Tinline \
  -destination 'platform=iOS Simulator,name=iPhone 15' build 2>&1 | tee ios-app.log
```

(Any installed simulator name works: `xcrun simctl list devices`.) To run it, open
`ios/Tinline.xcodeproj` in Xcode and press Run with a simulator selected. Signing is not
configured; for a real iPhone set your team under Signing & Capabilities. Camera, CallKit, push
and QR scanning are stubs for now.

## What to send back

The three logs (`desktop-build.log`, `ios-core.log`, `ios-app.log`), from wherever each one
stopped, plus `sw_vers`, `xcodebuild -version` and `uname -m`. Screenshots if anything launched.
Please don't fix errors by hand unless it's trivial; the logs are what we need.
