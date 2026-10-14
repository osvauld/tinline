# Where the desktop and Android copies differed

Sources: **dev** = the code in this branch's base (`refactor/main`, 698bcb1). **ui-parity** = branch
`ui-parity/main` (7f902e8), which has newer UI code. `present` follows ui-parity where it differs from dev.
Android `K/` = `android/app/src/main/kotlin/com/osvauld/p2p/`, desktop `D/` = `crates/desktop/src/`.
VOICE.md is only in ui-parity (`docs/design/VOICE.md`); it says nothing about the items below except where noted.

| Item | Desktop (file:line, behaviour) | Android (file:line, behaviour) | Chosen | Why | Needs user OK |
|---|---|---|---|---|---|
| Initials | dev `D/ui.rs:652`: first + second word ("Ann Marie Lee" = AM). ui-parity `D/ui.rs:719`: first + last word (comment cites playground `store.svelte.ts initials()`) | dev and ui-parity `K/Components.kt:187`: first + last word | first + last ("AL") | Both apps on ui-parity agree, and it matches the playground. dev desktop was the odd one. | no |
| Avatar hash | dev `D/ui.rs:633`: `bytes * 31` as u32, `% 4`. ui-parity `D/ui.rs:705`: same hash `% 6`, plus an in-memory per-key hue override map (`HUES`). (The audit said `DefaultHasher`; that is only used for the raster cache key.) | dev `K/Components.kt:185`: UTF-16 `h*31+c` masked to 31 bits, `% 4`. ui-parity `K/Components.kt:200` + `ProfileAvatar.kt:42`: same, `% 6` | FNV-1a 32-bit over UTF-8 bytes (`avatar_slot(key, slots)`, apps pass 6) | The brief asked for a documented stable hash. The two `*31` copies agreed only for ASCII keys with low bits; masked 31-bit vs u32 differ under `% 6`. **Side effect: every contact changes colour once.** Per-key hue override stays app-side. | yes (colours change; alternative: port the `*31` hash over UTF-8 bytes to keep today's colours) |
| Avatar slot count | 4 (dev) / 6 (ui-parity) | 4 (dev) / 6 (ui-parity) | caller passes `slots` | ui-parity uses 6 hue pairs | no |
| Call quality thresholds | `D/view.rs:~160`: 100/200/350 ms, loss 1/3/8 % | `K/Support.kt:65`: 120/250/450 ms, loss 1/3/8 % (comment cites a "tokens board") | desktop 100/200/350 | VOICE silent; desktop is the stricter and the audit's stated value | yes (Android bars get stricter) |
| End reason: cancelled | `D/reason.rs:37`: "Call cancelled" | `K/Support.kt:115`: incoming "X stopped calling", outgoing "You cancelled the call" | Android | Direction-aware; "You/your" voice | yes |
| End reason: no_answer, incoming | "No answer" | "Missed call" | Android | an unanswered incoming call is a missed call | yes |
| End reason: busy | `D/reason.rs:42`: "X is busy" | `K/Support.kt:118`: outgoing "X is on another call", incoming "Missed call" | Android | says what happened, no blame | yes |
| End reason: unavailable | `D/reason.rs:43`: "Turned away while unavailable" (ambiguous whose) | `K/Support.kt:122`: "Turned away while you were not available" | Android | clear subject | yes |
| End reason: blocked | `D/reason.rs:48`: always "X blocked you" (app overrides when ours) | `K/Support.kt:130`: `blocked_by_me` flag gives "You blocked X" | Android (flag) | one function covers both | no |
| Missed banner | `D/reason.rs:17`: `present()` returns `Missed("Missed call from X")`; `answered` flag stops cancelled/no_answer being missed after pickup | `isMissedReason(reason, incoming)` has no `answered`; `busy` incoming counts missed | desktop rule (with `answered`) in `end_view`; `is_missed_reason` takes `answered` | superset of both | no |
| Empty peer name | none ("X" becomes empty) | `K/Support.kt:107`: "They" | "They" | avoids blank text | no |
| Relative age | `D/view.rs:143`: "just now", "4 min ago", "3 h ago", "5 d ago", "2 w ago" | `K/Devices.kt:20`: "2 mins/3 hours/23 days ago"; `GroupsUi.kt:325`: "Just now", "2 h ago" | desktop compact form, lowercase "just now" | VOICE silent; desktop is used in most places and shorter | yes (Android wording changes) |
| Day label | `D/chat_view.rs:28`: Today/Yesterday/full weekday/"12 Sep"/"12 Sep 2025" if another year; a future date shows as a date | `K/ChatUi.kt:68`: same but no year; future = "Today" | desktop with year; future = "Today" | old dates with no year are ambiguous | no |
| List time | `D/chat_view.rs:40`: time today, else `day_label` (full weekday). `D/groups_view.rs:1149`: "Yesterday" else "Oct 12" | `K/ChatUi.kt:60`: time, "Yesterday", short weekday "Mon", "12 Sep" | desktop chat_view (`list_time`) | the real chat list on desktop | yes (Android shows "Monday" not "Mon") |
| When short / long | none | `K/History.kt:39,55` | Android | only copy | no |
| Clock | `D/voice.rs:361`: `m:ss`, rounds ms | `K/Support.kt:57`: zero-padded `mm:ss`; `K/VoiceMessage.kt:209`, `K/HomeLists.kt:118`: `m:ss`, `h:mm:ss` | `m:ss` / `h:mm:ss`, `clock_ms` rounds | 3 of 4 copies | no |
| Duration words | `D/reason.rs` history: clock only | `K/History.kt:65` "42 s/18 min/1 h 5 min"; `GroupsUi.kt:336` "2 h/3 days" | both kept (`duration_text`, `duration_words`) | different jobs | no |
| File size | `D/chat_view.rs` size_text: one decimal under 10 MB, whole MB above | `K/ChatUi.kt:81`: one decimal under 100 MB | desktop 10 MB | VOICE silent; desktop default | yes (Android "184.0 MB" vs "184 MB" cases only for 10-100 MB) |
| Emoji | `D/emoji.rs`, same ranges | `K/Emoji.kt`, same ranges; whitespace via `Character.isWhitespace` (not NBSP) | Rust `is_whitespace` (NBSP counts as space) | trivial edge | no |
| Link failure text | `D/link.rs:79` e.g. "Nobody approved in time. Nothing was linked.", "...this computer." | `K/Link.kt:166` e.g. "The other device didn't answer in time. Keep both open and try again.", "...this phone." | Android, with `this_device` parameter for the last line | VOICE "Error": what happened, then what to do; desktop lines often lack the next step | yes |
| QR check | `D/link.rs:292`: "That isn't a link code. It starts with OSVL1:" | `K/Link.kt:182`: separate message for contact codes (`OSVC`) and others; trims | Android | helps the common mix-up | no |
| Block dialog "add again" | `D/blocks.rs`: "...add you again with your code" | `K/Blocking.kt`: "...with your QR code" / "Add them again with their QR code" | desktop "code"; `not_contact_line` repeats the first name | desktop has no scanner; "code" fits both | no |
| Block extras | `D/blocks.rs:`: `CallGate::Closed(Composer)` carries the state | `K/Blocking.kt`: flat `Closed`; adds `cantSend`, `cantCall` | flat `CallGate`, Android extras kept | callers can call `composer_state` again | no |
| Display name fallback | none found | `K/History.kt:14`: "Unnamed" | "Unnamed" | only copy | no |
| Stats line | none for audio | `K/CallController.kt:362`: `stats secs=... ` | Android format (`audio_stats_line`) | only copy | no |

Not ported (outside the brief): chat grouping, group text, mark and geometry, `in_words`, `agoOrDate`, the history
`callWhat` labels, notification text.
