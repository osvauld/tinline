# present

Shared, pure UI logic for the iced desktop app and the Compose Android app: names and initials, avatar slot, call
end reasons and quality, time words, emoji runs, device-link text and blocking state. No clock, no zone, no
iroh/core dependency. Callers pass `now`, unix-second timestamps and a UTC offset (seconds east of UTC). Functions return
semantic enums plus default English text (voice: `docs/design/VOICE.md`).

Where the old copies differed, `DECISIONS.md` lists what was chosen and what needs a user decision.

Test: `tasks run --who <you> --anywhere -- cargo test -j 6 -p present`. Golden tables live in `src/<module>/tests.rs`.

## Wiring (done)

Desktop: `present = { path = "../present" }` in `crates/desktop/Cargo.toml`; call `present::names::initials(name)` and so on.

Android: `crates/core/Cargo.toml` depends on `present` and `crates/core/src/lib.rs` has `pub use present;`, which links the
crate and its UniFFI scaffolding into `libp2pcore.so`. The existing `uniffi-bindgen generate --library` step then writes
`uniffi/present/present.kt` (package `uniffi.present`) next to `uniffi/p2pcore/p2pcore.kt`; Gradle already adds the whole
generated directory to the Kotlin source set. Kotlin calls `uniffi.present.initials(name)`, `uniffi.present.clock(secs)` and so on.

JVM unit tests cannot load the native library, so Kotlin code that is unit-tested on the JVM must not call `uniffi.present`;
put its golden tests in `src/<module>/tests.rs` here instead.

Notes: UniFFI maps `u32`->`UInt`, `u64`->`ULong`, `i64`->`Long`, `i32`->`Int`; `Option<String>`->`String?`.
`pub const` is not exported, so constants that Kotlin needs are functions (`avatar_slot_count`, `emoji_inline_scale`, `unblock_note`).
