# Tinline for iOS (speculative)

Written on Linux, never compiled: no Mac was available. Expect first-build fixes.

- The UI is a SwiftUI port of the Android app (`android/.../p2p`) from the same playground designs. Logic is
  deliberately duplicated for now; shared pieces move into Rust once Android, iced and iOS can be compared.
- Generate the Xcode project on a Mac: `xcodegen generate --spec ios/project.yml` (needs `brew install xcodegen`).
- Core: `ios/scripts/build_core.sh` builds `p2pcore` for device + simulator and produces the XCFramework and the UniFFI Swift file.
- Signing, push/CallKit and Keychain are not configured (see the task list in the PR/HANDOFF).
