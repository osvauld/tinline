# present

Shared, pure UI logic for the iced desktop app and the Compose Android app: names and initials, avatar slot, call
end reasons and quality, time words, emoji runs, device-link text and blocking state. No clock, no zone, no
iroh/core dependency. Callers pass `now`, unix-second timestamps and a UTC offset (seconds east of UTC). Functions return
semantic enums plus default English text (voice: `docs/design/VOICE.md`).

Where the old copies differed, `DECISIONS.md` lists what was chosen and what needs a user decision.

Test: `python3 scripts/buildlock.py --who <you> -- cargo test -p present`. Golden tables live in `src/<module>/tests.rs`.

## Wiring it into the apps (not done yet)

Desktop (Rust): add `present = { path = "../present" }` to `crates/desktop/Cargo.toml` and call the functions directly
(`present::names::initials(name)`). The `String`/`Option<String>` signatures are the UniFFI ones; borrow-based
helpers are `names::first_word`.

Android, through the existing core cdylib:
1. `crates/core/Cargo.toml`: `present = { path = "../present" }`.
2. `crates/core/src/lib.rs`: after `uniffi::setup_scaffolding!();` add `pub use present;`. The re-export forces the
   crate (and its scaffolding symbols) to be linked into `libp2pcore.so`.
3. Rebuild the .so as today. The bindgen step is unchanged (confirmed with a scratch cdylib that does `pub use present;`):
   `uniffi-bindgen generate --library libp2pcore.so --language kotlin --out-dir <dir>`. Library mode reads the metadata of
   every UniFFI crate in the cdylib and writes one file per crate namespace, so it writes a new
   `uniffi/present/present.kt` (package `uniffi.present`) next to `uniffi/p2pcore/p2pcore.kt`.
4. Gradle: make sure the source set that collects the generated `uniffi/**/*.kt` takes the whole directory, not only
   `p2pcore.kt` (check `android/app/build.gradle.kts` and the generate task). Kotlin then calls
   `uniffi.present.initials(name)`, `uniffi.present.callQuality(rtt, loss)` and so on.
5. Then delete the Kotlin copies named in `DECISIONS.md`, and in desktop the matching copies.

Notes: UniFFI maps `u32`->`UInt`, `u64`->`ULong`, `i64`->`Long`, `i32`->`Int`; `Option<String>`->`String?`.
`pub const` is not exported, so the unblock note is a function.
