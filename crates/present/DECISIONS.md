# Where the desktop and Android copies differed

Re-checked against `dev` 0f6a4393 (2026-10-20). Android `K/` = `android/app/src/main/kotlin/com/osvauld/p2p/`,
desktop `D/` = `crates/desktop/src/`. `present` is linked into both apps (desktop: direct dependency; Android: re-exported by
`p2pcore` so it lands in `libp2pcore.so` as package `uniffi.present`).

**Status column.** *Wired*: the app copies are deleted and both apps call `present`. *Waiting*: the row needs a user decision (none left). *Open*: the apps now differ from `present` (or from each
other) in a way the old table did not cover; nobody picked.

| Item | Desktop now | Android now | Chosen | Needs user OK | Status |
|---|---|---|---|---|---|
| Initials | first + last word (was `D/ui.rs:770`) | first + last word (was `K/Components.kt:185`) | first + last ("AL") | no | Wired |
| Avatar slot count | 6 | 6 | `AVATAR_SLOTS` / `avatar_slot_count()` = 6 | no | Wired (count only) |
| Avatar hash | FNV-1a 32 over UTF-8 via `present::avatar::avatar_slot` | same (`uniffi.present.avatarSlot`) | FNV-1a 32 over UTF-8, 6 slots | decided 2026-10-20 (user): every contact changes colour once | Wired |
| Call quality thresholds | `present::call::call_quality` | same | 100/200/350 ms, loss 1/3/8 %; words Strong/Good/Fair/Weak (what both apps already said; `present` used Excellent/OK/Poor before) | decided 2026-10-20 (user) | Wired |
| End reason: cancelled | `present::call::end_view` | same | incoming "X stopped calling", outgoing "You cancelled the call" | decided 2026-10-20 (user) | Wired |
| End reason: no_answer, incoming | same | same | "Missed call" | decided 2026-10-20 (user) | Wired |
| End reason: busy | same | same | outgoing "X is on another call", incoming "Missed call" | decided 2026-10-20 (user) | Wired |
| End reason: unavailable | same | same | "Turned away while you were not available" | decided 2026-10-20 (user) | Wired |
| End reason: blocked | `blocked_by_me` flag | `blocked_by_me` flag | flag | no | Wired |
| Missed banner | `present::call::is_missed_reason` with `answered` | same | desktop rule | no | Wired |
| Empty peer name | "They" | "They" | "They" | no | Wired |
| Relative age | `present::time::ago` | same | "just now", "4 min ago", "3 h ago" | decided 2026-10-20 (user) | Wired |
| Day label | `D/chat_view.rs` `day_label` (list time and day grouping); the day chip `chip_day` ("Thursday 8 Oct") is separate | `K/ChatUi.kt:56` `dayLabel` is the chip form ("Thursday 8 Oct"), not the weekday-or-date form | desktop `day_label` | no | Wired on desktop only. **Open**: Android's `dayLabel` is the chip form, the same as desktop's `chip_day`, so `present::time::day_label` has no Android caller. The chip form is not in `present` |
| List time | `present::time::list_time` | same | time today, "Yesterday", full weekday ("Monday"), "12 Sep" | decided 2026-10-20 (user) | Wired |
| When short / long | none | `K/History.kt` (now thin wrappers that pass the zone) | Android | no | Wired |
| Clock | `m:ss` / `h:mm:ss`, rounds ms | `K/Support.kt` live call timer, `K/HomeLists.kt`, `K/VoiceMessage.kt` | `m:ss` / `h:mm:ss` | no | Wired for the copies named in the old table. **Open**: zero-padded `mm:ss` copies remain (see list in the task report): `D/call_screen.rs`, `D/gcall_view.rs`, `D/view.rs` call card and ended card, `K/GroupCallUi.kt::clockOf`; and unpadded copies without hours: `D/view.rs:866`, `D/chat_view.rs:534`, `K/Conversation.kt:552` |
| Duration words | none | `K/History.kt` `durationText` (wired); `duration_words` has no app copy any more | both kept | no | Wired |
| File size | `present::time::bytes` | same | one decimal only below 10 MB | decided 2026-10-20 (user) | Wired |
| Emoji | `D/emoji.rs` | `K/Emoji.kt` | Rust `is_whitespace` | no | Wired (both apps call `present::emoji`) |
| Link failure text | `present::link::link_fail_text` (`this_device` = "computer") | same ("phone") | Android wording with the what-to-do lines | decided 2026-10-20 (user) | Wired |
| QR check | `D/link.rs:289` | `K/Link.kt` | Android | no | Wired |
| Block dialog wording | "code" | "QR code" | "code" | no | Wired |
| Block extras | inline in `D/app.rs` | `BlockCopy.cantSend` / `cantCall` | flat `CallGate`, Android extras | no | Wired |
| Display name fallback | none | `K/History.kt` | "Unnamed" | no | Wired (Android) |
| Stats line | none | `K/CallController.kt` | Android format | no | Wired (Android) |

`MainActivity.kt` still calls `Contact.display()`, `firstName` and `BlockCopy.cantCall`, so those stay as thin Kotlin wrappers.
The Kotlin `Blocks` model (`composerState`, `callGate`) stays too: JVM unit tests cannot load the native library.

Not ported (outside the brief): chat grouping, group text, mark and geometry, `in_words`, `agoOrDate`, the history
`callWhat` labels, notification text.
