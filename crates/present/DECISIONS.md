# Where the desktop and Android copies differed

Re-checked against `dev` 0f6a4393 (2026-10-20). Android `K/` = `android/app/src/main/kotlin/com/osvauld/p2p/`,
desktop `D/` = `crates/desktop/src/`. `present` is linked into both apps (desktop: direct dependency; Android: re-exported by
`p2pcore` so it lands in `libp2pcore.so` as package `uniffi.present`).

**Status column.** *Wired*: the app copies are deleted and both apps call `present`. *Waiting*: the row needs a user decision;
the app copies stay as they were and `present` is not called for it. *Open*: the apps now differ from `present` (or from each
other) in a way the old table did not cover; nobody picked.

| Item | Desktop now | Android now | Chosen | Needs user OK | Status |
|---|---|---|---|---|---|
| Initials | first + last word (was `D/ui.rs:770`) | first + last word (was `K/Components.kt:185`) | first + last ("AL") | no | Wired |
| Avatar slot count | 6 | 6 | `AVATAR_SLOTS` / `avatar_slot_count()` = 6 | no | Wired (count only) |
| Avatar hash | `D/ui.rs:756` `bytes*31` as u32 `% 6`; per-key hue map | `K/Components.kt:180` UTF-16 `h*31+c` masked to 31 bits `% 6` | FNV-1a 32 over UTF-8 (`avatar_slot`) | yes: every contact changes colour once; alternative: port the `*31` hash over UTF-8 bytes | Waiting (apps keep their hash) |
| Call quality thresholds | `D/call_ui.rs:18` 100/200/350 ms, loss 1/3/8 % | `K/Support.kt:76` 100/200/350 ms, loss 1/3/8 % | thresholds agree now | no longer a difference | Waiting on the words: both apps say Strong/Good/Fair/Weak (`D/call_ui.rs:32`, `K/SignalBars.kt:65`), `present` says Excellent/Good/OK/Poor |
| End reason: cancelled | `D/reason.rs` "Call cancelled" | incoming "X stopped calling", outgoing "You cancelled the call" | Android | yes | Waiting (desktop keeps its words for these four tokens in `D/reason.rs::text`; Android calls `present`, same text as before) |
| End reason: no_answer, incoming | "No answer" | "Missed call" | Android | yes | Waiting (as above) |
| End reason: busy | "X is busy" | outgoing "X is on another call", incoming "Missed call" | Android | yes | Waiting (as above) |
| End reason: unavailable | "Turned away while unavailable" | "Turned away while you were not available" | Android | yes | Waiting (as above) |
| End reason: blocked | `blocked_by_me` flag | `blocked_by_me` flag | flag | no | Wired |
| Missed banner | `present::call::is_missed_reason` with `answered` | same | desktop rule | no | Wired |
| Empty peer name | "They" | "They" | "They" | no | Wired |
| Relative age | `D/view.rs:156` "4 min ago", "3 h ago" | `K/Devices.kt:21` "2 min ago", "3 hours ago", "23 days ago" | desktop compact form | yes | Waiting |
| Day label | `D/chat_view.rs` `day_label` (list time and day grouping); the day chip `chip_day` ("Thursday 8 Oct") is separate | `K/ChatUi.kt:56` `dayLabel` is the chip form ("Thursday 8 Oct"), not the weekday-or-date form | desktop `day_label` | no | Wired on desktop only. **Open**: Android's `dayLabel` is the chip form, the same as desktop's `chip_day`, so `present::time::day_label` has no Android caller. The chip form is not in `present` |
| List time | `D/chat_view.rs:46` time today else `day_label` | `K/ChatUi.kt:47` time, "Yesterday", short weekday, "12 Sep" | desktop | yes | Waiting |
| When short / long | none | `K/History.kt` (now thin wrappers that pass the zone) | Android | no | Wired |
| Clock | `m:ss` / `h:mm:ss`, rounds ms | `K/Support.kt` live call timer, `K/HomeLists.kt`, `K/VoiceMessage.kt` | `m:ss` / `h:mm:ss` | no | Wired for the copies named in the old table. **Open**: zero-padded `mm:ss` copies remain (see list in the task report): `D/call_screen.rs`, `D/gcall_view.rs`, `D/view.rs` call card and ended card, `K/GroupCallUi.kt::clockOf`; and unpadded copies without hours: `D/view.rs:866`, `D/chat_view.rs:534`, `K/Conversation.kt:552` |
| Duration words | none | `K/History.kt` `durationText` (wired); `duration_words` has no app copy any more | both kept | no | Wired |
| File size | `D/chat_view.rs:53` one decimal under 10 MB | `K/ChatUi.kt:72` one decimal under 100 MB | desktop 10 MB | yes | Waiting |
| Emoji | `D/emoji.rs` | `K/Emoji.kt` | Rust `is_whitespace` | no | Wired (both apps call `present::emoji`) |
| Link failure text | `D/link.rs` | `K/Link.kt:164` | Android | yes | Waiting |
| QR check | `D/link.rs:289` | `K/Link.kt` | Android | no | Wired |
| Block dialog wording | "code" | "QR code" | "code" | no | Wired |
| Block extras | inline in `D/app.rs` | `BlockCopy.cantSend` / `cantCall` | flat `CallGate`, Android extras | no | Wired |
| Display name fallback | none | `K/History.kt` | "Unnamed" | no | Wired (Android) |
| Stats line | none | `K/CallController.kt` | Android format | no | Wired (Android) |

`MainActivity.kt` still calls `Contact.display()`, `firstName` and `BlockCopy.cantCall`, so those stay as thin Kotlin wrappers.
The Kotlin `Blocks` model (`composerState`, `callGate`) stays too: JVM unit tests cannot load the native library.

Not ported (outside the brief): chat grouping, group text, mark and geometry, `in_words`, `agoOrDate`, the history
`callWhat` labels, notification text.
