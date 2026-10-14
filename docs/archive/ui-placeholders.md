# Retired placeholder UI (2026-10-14)

The groups & privacy PREVIEW screens ran on invented data before the core had groups. The core now
has real groups (`group_live*.rs` on desktop, `GroupsLive.kt` on Android), so the placeholder layer
was removed from shipped builds. Everything is restorable from the git tag
`archive/pre-retire-2026-10-14`.

## Desktop (`crates/desktop/src`)

| File | Fate |
|---|---|
| `groups_fake.rs` (placeholder state, ~1.7k lines, 9 tests) | Compiled only under `cfg(any(test, feature = "test-hooks"))`. Not in release. |
| `groups.rs` (preview UI state, `Gm` messages) | Same gate. `PREVIEW_DEFAULT` is now `cfg!(feature = "test-hooks")` (was also on in every debug build). |
| `groups_preview_view.rs` (new; the preview screens, split out of `groups_view.rs`) | Same gate. Holds the group chat, member menu, contact panel, Hosting page, dialogs, and the Privacy "Preview" switch. |
| `groups_view.rs` | Now only what live screens share: time formats, rule, tag, footer, `sys`, `btn`, `ordinal`, `short_time`, `message_body`, and the real Settings > Privacy "Blocked people" page. |

Hooks in `app.rs`, `view.rs`, `chat_view.rs`, `signup.rs`, `main.rs` carry the same cfg. The design
screenshots (`P2P_DEMO=groups*`, `demo.rs`) are test-hooks builds, so they keep working unchanged.

## Android (`android/app/src`)

Moved from `main` to `debug` (so only the debug gallery, `GalleryActivity` `pv_*`, can show them):
`FakeGroupsPrivacy.kt`, `GroupsSource.kt`, `GroupsScreens.kt`, `PrivacyScreens.kt`, `PreviewNav.kt`,
`PreviewFlags.kt` (with `PreviewBackend`), `GateCompat.kt` (old gate screens used only by the gallery),
and the preview-only helpers of `GroupsUi.kt` (now `debug/.../GroupsPreviewUi.kt`).

Removed from `main`: the "Preview groups & privacy" switch and the preview rows in `Settings.kt` and
`SettingsRoutes.kt`, the `Route.Preview` route in `MainActivity.kt`, and the unused `onPreview`
parameter of `Home.kt`. Debug builds therefore no longer show the in-app switch; use
`--es screen pv_<name>` on the gallery.

Kept in `main/GroupsUi.kt` because `GroupsLive.kt` uses them: `GroupAvatar`, `PersonAvatar`, `FlagMeter`,
`SysLine`, `BlockedFooter`, `NoticeCard`, `authorColor`, `whenFull`, `dayAndTime`. Two small changes
there: the fake DIDs (ARJUN, SAM, ...) no longer pick avatar hues or author colours, and `FlagMeter`
defaults to `GroupText.FLAGS_TO_REMOVE`. Real DIDs behave as before. In the debug gallery the preview
people now get the normal hash-based hue.

## Kept on purpose

- Desktop Settings > Privacy "Blocked people" list and Android `BlockedPeopleScreen`/`BlockDialog` (in
  `Blocking.kt`): live, core-backed.
- `FakeGroupsPrivacyTest.kt` stays in `src/test`; it runs with `testDebugUnitTest` because the fake is
  in the debug source set.
- No placeholder-only feature was found whose only UI was in the removed code: address-change
  (rotation) settings and chat-request UI have no live screen yet, but they were reachable only
  through the preview switch, which was off by default and debug-only on Android. They are now
  reachable only in test-hooks builds (desktop) or the debug gallery (Android). Rebuild them on the
  core API when rotation UI is scheduled.

## Restore

```
git checkout archive/pre-retire-2026-10-14 -- crates/desktop/src/groups_view.rs crates/desktop/src/app.rs \
  crates/desktop/src/view.rs crates/desktop/src/chat_view.rs crates/desktop/src/signup.rs crates/desktop/src/main.rs
git rm crates/desktop/src/groups_preview_view.rs
git checkout archive/pre-retire-2026-10-14 -- android/app/src/main
git rm -r android/app/src/debug/kotlin/com/osvauld/p2p/{FakeGroupsPrivacy,GroupsSource,GroupsScreens,PrivacyScreens,PreviewNav,PreviewFlags,GateCompat,GroupsPreviewUi}.kt
```

To only re-enable the desktop preview in a normal build, change the five
`cfg(any(test, feature = "test-hooks"))` gates back to nothing.
