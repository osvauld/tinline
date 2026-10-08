# Chat media: what is built, 2026-10-08

Status: **built** (in code, design board shows it), **partial**, **designed only** (board shows it, no code), **missing** (neither). "Layout fix" = the uncommitted bubble-width work in the `chat-media-ui` worktree (not yet on `dev`); lines for it are in that worktree.

Paths: `A` = `android/app/src/main/kotlin/com/osvauld/p2p/`, `D` = `crates/desktop/src/`.

| Feature | Android | Desktop | Design board |
|---|---|---|---|
| Attach: photo, camera, file | built: `A Conversation.kt:229-235` (pickers), sheet `:573` | built: file picker `D chat.rs:601`, drag and drop `:616` | AndroidChatFiles "Attach" (built); DesktopChat picker frame (built) |
| Send progress, waiting, retry | built: `Conversation.kt:448-455`, `:488-490` | built: `D chat_view.rs:431-447` | AndroidChatFiles "Sending", "Receiving"; Retry shown in "Save only · failures" |
| Download on tap, paused when peer offline | built: `Conversation.kt:510-519` | built: `chat_view.rs:440-446`, `:449-450` | AndroidChatFiles "Receiving" |
| "Files over 25 MB wait" setting | missing in UI: core has `auto_download_limit` (`crates/core/src/chat/api.rs:203-208`), no app calls it | missing in UI (same) | designed only: hint text in "Receiving", marked Not built yet |
| Image thumbnail in bubble | built: `Conversation.kt:474-494` (240x180, tap to open) | built: `chat_view.rs:511-525`, decode `media.rs:235` (640x720 max) | Android "Receiving"/"Sending"; Desktop "Images and documents in bubbles" |
| Undecodable image falls back to file row | built: `Conversation.kt:478-481` | built: `chat_view.rs:461-471` (Bad thumb shows as file) | AndroidChatFiles "Save only · failures" |
| Full-size image viewer | partial: `Conversation.kt:596`, fit to screen, Share and Save; no zoom, no swipe between photos | partial: `chat_view.rs:548-624`, fits window to 2048 px, previous/next (arrows, Esc), no zoom or pan | Android "Photo viewer"; Desktop "Image viewer overlay"; zoom marked Not built yet |
| PDF in-app | built: `A FileViewer.kt:53-91` (PdfRenderer), viewer `:92` | missing: PDF goes to the default app (`D media.rs:49`, `chat.rs:811`) | AndroidChatFiles "PDF viewer"; desktop listed under Still needed |
| Text viewer (.txt .md .csv .log, 1 MB) | built: `FileViewer.kt:143-147`, limit `A FileOpen.kt:19` | built: `D media.rs:41-46`, `:258`, overlay `chat_view.rs:580` | "Text viewer" (Android), "Text viewer overlay" (Desktop) |
| "Open with" system app (Office, audio, video) | built: `FileOpen.kt:65-76`, chooser; message when no app: `Conversation.kt:224` | built: default app only for the allowlist, `media.rs:268`, `chat.rs:830` | AndroidChatFiles "Open with"; Desktop chips Open and Save |
| Save only for unsafe types (APK, mismatches) | built: `FileOpen.kt:56-63` (ext and MIME must agree), `Conversation.kt:220` | built: `media.rs:96-109`, tests `:290`, `:319` | AndroidChatFiles "Save only · failures"; Desktop bubbles (setup-tool.apk) |
| Private decrypted cache | built: `ChatMedia.decrypt`, wiped by `ChatMedia.clear` | built: `media.rs:127-200`, wiped at start and quit | text on boards only |
| Save to phone / computer | built: `Conversation.kt:208-214` | built: `chat.rs:643-659` | "Photo viewer", Desktop chips |
| Voice messages (record, send, play) | built: `A VoiceMessage.kt:447`, bubble `Conversation.kt:624` | built: `D voice.rs:440` | AndroidChatFiles "Hold to record" (partly: lock gesture not checked); Desktop recording bar |
| Bubble width rule | built in worktree only: `min(82%, 480dp)`, content width (`chat-media-ui` `Conversation.kt:389-408`); `dev` still has `290.dp` (`:426`) | built in worktree only: shrink-sized metadata row, 88 px hover slot (`chat-media-ui` `chat_view.rs:320,366`); `dev` max_width 520 (`:357`) | AndroidConversation, AndroidChatFiles "Bubble width", Desktop "Attach picker and bubble width" |
| Audio / video playback in-app | missing (opens in another app) | missing (opens in default app) | designed only as file rows |
| Cancel or pause a transfer | missing (no control in `FileRow`) | missing | AndroidChatFiles ring shows a cancel X on the sending photo: designed only |
| Real device test | pending (TASKS 31) | pending (TASKS 31) | n/a |

## What more we need, in order

1. Real phone to laptop test of images, PDF, text and Open for each side (TASKS 31 left it pending), including a few hostile files (renamed APK, double extension, odd bidi name).
2. Merge the bubble layout fix from `chat-media-ui` into `dev`; the boards already show it.
3. Cancel an active transfer (ring X on Android photo, button on desktop), and restart a stuck one. The Android board draws the X; no code has it.
4. Settings control for the auto-download limit on both apps (core already stores it). Today the 25 MB hint on the board is not true in the app.
5. Photo zoom and pinch on Android, and zoom/pan on desktop; swipe between photos on Android.
6. PDF viewer on desktop, so both platforms match (or an explicit decision to keep the default app).
7. In-app audio and video player, or leave as Open with and say so in the design.
8. Password prompt for protected PDFs; an animated GIF in the viewer.
9. Paste an image from the clipboard on desktop; share-into-Tinline on Android.
