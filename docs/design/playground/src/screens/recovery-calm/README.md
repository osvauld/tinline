# Recovery & sharing — proposed interaction study

Dedicated worktree: `.claude/worktrees/recovery-calm`, branch `design/recovery-calm`.
Android contact image import and Share are implemented in this worktree. The
recovery/profile layout is still a proposal. Nothing here is committed, merged
or installed on a phone yet.

Preview: http://127.0.0.1:5180/src/screens/recovery-calm/index.html

This standalone entry avoids editing lead-owned shell/router/motion files. It
uses shared motion/theme/reduced-motion tokens, the account store and avatar,
the existing people-store contact code and the shared synthetic recovery fixtures.
Its isolated bootstrap sets the sample username to `maya` through the existing
profile action; it does not alter the shared seed or any real account.

## Flows

- **Restore account:** key or 24 words → identity preview → optional local
  passphrase → end of this preview. Actual native device naming, terms and
  permission steps are not removed by this mockup.
- **Recovery in Profile:** proposed private shortcut → passphrase or simulated
  screen-lock check → hidden backup → explicit reveal. Key copying requires
  both authentication and a currently visible key. Words have no copy action
  in this study; native clipboard policy remains a separate integration concern.
- **Copy my code:** one short lead, Copy and Share, optional sample QR. Clipboard
  refusal never says “Copied”. Share passes only the bare contact fixture to
  Web Share, or offers Copy where unsupported. User cancellation is quiet and
  opening a chooser is never reported as delivery. Recovery has no Share action.
- **Choose image:** simulated OS picker with contact/no-QR/multiple-QR/cancel
  paths. A found sample opens a username preview. Only explicitly pressing Add
  records a simulated pending request in the shared people store. Android uses
  a real system picker and local QR decoding instead of this fixture gallery.
  The code's lifecycle and contact-acceptance rules are unchanged.

## User feedback

- Names are **one user-chosen username**, not first and last names. All screens
  read the same account value intact. No `@` prefix, global uniqueness guarantee
  or new character restrictions are invented. The app's internal field is still
  named `name`; this lab changes neither the schema nor the identifier/DID.
- Sharing/QR should fit the standard phone frame without scrolling. Compact
  identity and spacing replace the oversized fixed-height layout; the extra
  poetic tagline and inline code explanation were removed. This does not hide
  a scrollbar or clip overflowing content: scrolling remains available for
  genuinely long content, expanded help and enlarged text.
- The outer browser document can scroll to lab controls/notes; it is not the
  proposed shipped phone screen.

## Safety and limitations

**Never enter real secrets here.** Only exact shared demo fixtures are accepted.
There is no native core, BIP39/checksum verification, hardware authentication,
secure-window flag, or actual account restoration. The username shown after a
sample restore is shared fixture data, not information extracted from the key.

Secret form drafts clear on navigation. Backups hide after a real 30-second
security timeout and re-lock on browser focus loss/visibility changes. These
browser safeguards do not establish native secret-handling guarantees.

The key restores identity, not message history by itself. On a phone with
existing phrase-keyed stores, native recovery may reopen those local files;
legacy unreadable files must be preserved. No reset/wipe is proposed.

## Verification

From the repository root, with the isolated Vite server running:

```sh
python3 scripts/buildlock.py --who recovery-calm-layout-browser --run -- \
  node .claude/worktrees/recovery-calm/docs/design/playground/src/screens/recovery-calm/verify.cjs
```

The suite covers desktop (1440×900) and Android-harness (600×900), light/dark;
fixture restoration in both formats; authentication failures; explicit reveal;
clipboard success/failure; no cross-copy of contact/recovery data; background
locking; the actual 30-second hide timer; RTL/reduced motion; 44px controls; and
360px document overflow. It asserts that standard sharing and QR content fits
without internal scrolling and that `maya` is displayed intact.

The suite also checks Share payload isolation, unsupported/cancelled/refused
sharing, gallery cancellation and error paths, and no request before confirmation.

Type checks are scoped to `src/screens/recovery-calm`, followed by both existing
TypeScript projects. Full `npm run check` is also run to expose inherited shared
screen diagnostics rather than modifying another area's files.

Logs: `/tmp/recovery-calm-layout-types.log`,
`/tmp/recovery-calm-layout-browser.log`. Screenshots and frame reports stay under
`/tmp`, outside Git.

Prior recovery/layout revision results: browser suite **PASS**; scoped Svelte **0 errors, 0 warnings**;
both TypeScript projects passed; `git diff --check` passed. Full `npm run check`
remains blocked by the inherited **4 errors and 6 warnings** in `CallsList.svelte`,
`Gate.svelte` and `ContactsList.svelte`; those shared files were not edited.

Visuals reviewed in Android/desktop, light/dark:
`/tmp/claude-1000/-home-abe-p2p-chat/6f1f15a0-05df-4e79-8378-c444aaf8d38e/scratchpad/pg/recovery-calm/1792050295278-*-share.png`.
Report: `/tmp/recovery-calm-1792050295278-report.json`.
No phone operations or native recovery tests were run for the lab revision.

## Android image import and contact sharing

Owned files:
- `android/app/src/main/kotlin/com/osvauld/p2p/AddContactSheet.kt`
- `android/app/src/main/kotlin/com/osvauld/p2p/ContactQrImagePicker.kt`
- `android/app/src/main/kotlin/com/osvauld/p2p/ContactQrDecoder.kt`
- `android/app/src/test/kotlin/com/osvauld/p2p/ContactQrDecoderTest.kt`

Choose image uses AndroidX's system photo picker (with the library's system
fallback), no broad storage/gallery permission. Android's ImageDecoder handles
image orientation and bounds the software bitmap to 2048px per side before
allocating a pixel buffer. Existing ZXing decodes QR locally, in both polarities.
No new dependency or Google Play Services requirement is added. Blank,
unreadable and ambiguous multi-QR images get quiet hints. Repeated copies of the
same QR are not ambiguous.

Decoded text is untrusted and goes through the same core `peekCode` validation
as pasted/camera codes. It never dispatches a URL or automatically adds anyone.
The preview says “Read from image”, not “Scanned in person” or “verified”. Camera
and image results are guarded against stale input/navigation. The full username
is retained, with short generic Add/Message actions instead of splitting names.

Share reuses `shareCard` and Android's ACTION_SEND/text/plain chooser with the
bare current contact code. It does not share the recovery key or a QR image file.
A chooser opening is not a guarantee of sending, acceptance or connection.

Native validation (under the shared build queue, from this worktree's `android/`):

```sh
JAVA_HOME=/usr/lib/jvm/java-21-openjdk ANDROID_HOME=/home/abe/Android/Sdk \
./gradlew --console=plain :app:testDebugUnitTest \
  --tests com.osvauld.p2p.ContactQrDecoderTest \
  -x cargoNdkBuild -x uniffiBindgen -Pabis=arm64-v8a
```

**10 tests passed, 0 skipped**, including a synthetic ticket, inverted QR,
rotation, a QR within a larger screenshot, different/repeated QRs, blank images,
non-contact QR text, malformed pixel buffers and memory dimensions.
`compileDebugKotlin` and `compileReleaseKotlin` passed. The initial release-unit
attempt failed because shared tests import debug-only FakeGroupsPrivacy; no
unrelated tests/sources were edited to work around it. The corrected debug
variant compiles the normal test sources and selects only these 10 tests.

Native generation was excluded: this worktree reuses ignored, copied UniFFI and
ARM64 native outputs from the successful merged-dev build at the same base
`799d062`. This validates the Kotlin integration but is not a fresh Rust build.
No real photo-picker/share-chooser device test or phone installation has run.

The ARM64 APK build **passed** using the same environment and exclusions:

```sh
./gradlew --console=plain :app:assembleRelease \
  -x cargoNdkBuild -x uniffiBindgen -Pabis=arm64-v8a
```

APK: `.claude/worktrees/recovery-calm/android/app/build/outputs/apk/release/app-release.apk`
SHA-256: `f1b1de336e3d61f1d404d1680a38d7e9103f74a28410f1f7a8cecd896cc2035e`.
No installation was performed.

The queued lab check runs `npm run check`, then:

```sh
./node_modules/.bin/svelte-check --workspace src/screens/recovery-calm --no-tsconfig --fail-on-warnings
./node_modules/.bin/tsc --noEmit -p tsconfig.app.json
./node_modules/.bin/tsc -p tsconfig.node.json
```

Native logs: `/tmp/contact-qr-native-check.log`,
`/tmp/contact-qr-debug-unit.log`, `/tmp/contact-qr-release-apk.log`.
Latest browser report: `/tmp/recovery-calm-1792052737135-report.json` (**PASS**),
with light/dark Android/desktop sharing, QR and simulated gallery screenshots
under the same scratch directory. Browser log: `/tmp/contact-qr-lab-browser.log`.
The new QR/Share full/scoped type check is still waiting for shared compile
admission (PID 2717583); it has not been bypassed or duplicated. Its eventual
results are in `/tmp/contact-qr-lab-types.log`. Do not interpret the prior
revision's clean scoped results as a completed type check of these additions.

## Android profile and recovery (implemented)

Profile (`Route.Me`) shows name, avatar, Edit profile, "Share my code" and a
private "Account recovery" row. Edit profile and Share my code are their own
routes (`Route.EditMe`, `Route.MyCode`). "Account recovery" opens the gate
(app passphrase, or screen lock when none is set) then the hidden backup
(Key | 24 words, explicit Reveal, 30 s auto-hide, key copy only while visible,
no copy for words, no Share). Settings > Privacy has the same "Account recovery"
entry to the same gate.

Relock: losing window focus on the backup screen replaces it with the gate
(the words leave memory and the next look needs the check again), rather than
only hiding in place. The focus listener lives only on the backup screen, so the
screen-lock/biometric prompt started from the gate cannot trigger a relock loop;
the gate also guards its own ON_PAUSE with a `prompting` flag.

FLAG_SECURE: `secureWindow()` runs in `MainActivity.onCreate` for the whole
activity (gate, backup, profile), release builds only (debug stays capturable by
design). Unchanged by this work.

Files: Settings.kt, SettingsRoutes.kt, SettingsPages.kt, AccountProfile.kt,
RecoveryBackupState.kt, ProfileScreens.kt, MainActivity.kt, debug
GalleryActivity.kt (`pf_account`, `pf_code`, `pf_code_failed`), test
RecoveryBackupStateTest.kt.

Validation (all through buildlock, `--offline`, `-x cargoNdkBuild -x uniffiBindgen
-Pabis=arm64-v8a`, reusing existing UniFFI/native outputs):
- `:app:testDebugUnitTest --tests *ContactQrDecoderTest --tests *RecoveryBackupStateTest`:
  BUILD SUCCESSFUL; 10 + 5 tests passed, 0 skipped/failed.
- `:app:compileReleaseKotlin`: BUILD SUCCESSFUL (only a pre-existing
  LocalLifecycleOwner deprecation warning in MainActivity.kt:179).
- Lab: scoped svelte-check 0 errors 0 warnings; both tsc projects OK;
  verify.cjs PASS (server on 5180 serves this worktree).

Not tested: photo picker, share chooser, screen-lock/biometric prompt, relock on
a device; no emulator or phone was used.
