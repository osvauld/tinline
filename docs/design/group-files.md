# Group files: any file up to 2 GB in a group

Status: core, Android and desktop UI built on `feat/group-files` (2026-10-30). iOS UI not built
(`docs/TODO.md`, "Group files"). Who keeps the bytes (§4): the user chose **option B** for now.

User decision: a group post can carry a file of any type up to 2 GB, as 1:1 chats already can.
This reuses what exists: the 1:1 chunked blob cipher (`transfer::crypt`, `MAX_FILE`), the group
blob store and its gated iroh-blobs serving on the group endpoint (`engine/images.rs`, which
group pictures and voice posts already use), and the group auto-download setting. Nothing here
builds a second transfer system.

## 1. Constraints this design rests on

- **Members never dial members (U7).** Group traffic goes through hosts. Only hosts know
  member group endpoints. Two members can connect directly only if they are also contacts with
  a consent-based link (`docs/protocol.md` "Session lifetime"), and that link runs on the
  private endpoint, not the group endpoint. So "fetch from the sender or another member" can
  only work in a group through a host.
- **Hosts are often phones.** Storing every 2 GB file on every host is heavy.
- **Every device reaches the same verdict on a record (U10/U22).** File limits are checked in the
  record, on every device.
- **Old apps** refuse a batch that holds a record they cannot decode (see §8).

## 2. Records

Two new `Body` variants, added after `Voice` (wire tags 15 and 16), so older records decode
unchanged.

```text
Body::File { caption: String, reply_to: Option<String>, file: FileRef }
FileRef { hash: [u8; 32], key: [u8; 32], size: u64, name: String, mime: String }

Body::FileHeld { post: String, day: String, hash: [u8; 32], held: bool }
```

- `File` is a post (`Body::is_post`). It goes in day docs and is judged by U11 visibility,
  admins-only and removal like `Post` and `Voice`. The list row and notifications say
  "File: <name>" (UI).
- `FileRef.hash` is the BLAKE3 (bao) root of the sealed blob. The sender computes it while
  sealing the file, before the post exists, and the author signs it with the post. This is the
  same thing a 1:1 file message commits to (`MsgFile.hash`). `key` is a fresh random key for this
  one blob (`transfer::crypt`: AES-256-GCM in 64 KiB chunks, the chunk index in the nonce, a "last"
  flag in the AAD). `size` is the plaintext length.
- Checks every device runs (`Record::check_shape`):
  - `size <= transfer::crypt::MAX_FILE` (2 GiB). An empty file is allowed.
  - `name` is 1 to 255 bytes, has no `/`, `\` or NUL, has no control characters, and is not `.`
    or `..`.
  - `mime` is at most 100 bytes and ASCII.
  - `caption` is at most `MAX_TEXT`, and `reply_to` at most 64 bytes.
- `FileHeld` is the holder announcement (§6). It is not a shown post. It lives in the **day doc
  of the file post it names** (`day`), so the post and its holders load together and an old app
  is affected only on days that already carry a file post (§8). `hash` must equal the post's
  `FileRef.hash`. If it does not match, the announcement is ignored. `day` is checked as a real
  calendar day. The doc rule (`doc::fits`) puts a `FileHeld` only in `DocId::Day(day)` and skips
  the ±1 h time window that posts have (its `at` is the time of the announcement).

## 3. How the bytes travel

- **Sending:** `group_post_file(group, path, name?, caption?)` streams the file from disk through
  the chunk cipher into a temporary file in the blob store (`BlobHub::add_file_owned`, 1 MiB
  buffers). It never holds the file in memory. Then it imports the file under the group's owner
  tag and writes the post. The sender's row is `ready` at once. No plaintext copy is kept: the
  sender's own "open" decrypts from the store, the same as for any receiver.
- **Verified streaming:** every download asks iroh-blobs for the signed `hash`. iroh-blobs checks
  each 16 KiB chunk group against the bao tree while it streams. A source that changes even one
  byte (a relaying host, a member that serves its copy) fails at that chunk, not after 2 GB
  (`transfer::Error::Corrupt`, "data does not match the signed hash"). Opening then checks the
  AEAD tag of every 64 KiB chunk as well. A corrupt download is given up at once on this
  device (`Failed`, tap tries again), its partial bytes are released, and it does **not** make
  this device a holder. Nothing that a holder announcement names is trusted except the signed
  hash.
- **Who serves:** the same gate as pictures. A blob is served only if a visible post of a group
  we hold names its hash, and the requester is, in that group, a current member on a session we
  host or an active host.
- **Member fetch:** from hosts only (live sessions first, then host addresses), as for pictures.
  Retries go 2/5/15/30/60 s and then stop (`Failed`). An attempt that got further than the last
  one (more verified bytes stored) resets the retry count, so a long download keeps
  going while it moves. Partial downloads resume.
- **Auto-download:** a file downloads by itself only if both of these allow it: the group
  auto-download mode (`group_image_auto_download`: Always / Wi-Fi / Tap), and the size limit the
  1:1 chat setting already has (`auto_download_limit`, default 10 MB, `0` = never). Bigger files
  wait for a tap (`group_fetch_file`). Hosts auto-fetch only the files they keep (§4).
- **Cancel:** `group_cancel_file` stops the transfer and releases the partial bytes (the store's
  GC frees them in about 30 s). The file goes back to `Remote`. This matches 1:1
  `cancel_download`.
- **Delete my copy:** `group_delete_file_copy` releases our blob and the opened copy. If we had
  announced that we hold the file, it writes `FileHeld{held: false}`.

## 4. Who keeps the bytes: option B (user decision, 2026-10-30)

Group pictures and voice posts are kept by every host, within a budget per group. Files can be
2 GB each. Three options were weighed. **The user chose B for now: it is the simplest, and we
will change it if storage becomes an issue.**

**B (current): hosts keep every file up to 2 GB.** `HOST_KEEP_FILE_MAX = MAX_FILE_BYTES` (one
constant in `group_core::record`).
- A host fetches every file post by itself, as soon as it sees it, like a picture (it ignores the
  member auto-download settings, as it does for pictures). It keeps the file within the group's
  media budget (`MAX_GROUP_IMAGE_BYTES`: 2 GiB plus the cipher tags of one 2 GiB file, so the
  largest file fits on its own), and eviction is the same as for pictures: blobs no visible post
  references go first, then the oldest.
- **Members** never download a big file by themselves: only within the 1:1 auto-download size
  limit (§3). Bigger files wait for a tap.
- Members fetch only from hosts (U7). A file is available whenever a host that has finished
  fetching it is online. Until then, and when no such host is online, the member's retries end
  `Failed` and the UI says "Not available right now. Try again later." A tap tries again, and
  so does the next session in the group.
- Cost: storage on every host, up to 2 GB per file. A 2 GB file evicts every older picture of
  the group on every host, and phones that host can fill up. This is the reason to revisit.
- There is no on-demand relay path. A relay as a fallback for a file a host has not finished
  fetching was considered and left out as extra complexity: it would be a second transfer path
  for the short window while a host is still fetching. (iroh-blobs can serve a partial blob's
  verified ranges, and member fetches resume, so a member asking during that window may get
  part of the file and the rest on a later attempt; this is not covered by a test.)

**User's ideas for later (not built):**
- (a) Only laptop/desktop hosts keep big files. Phone hosts keep pictures, voice and small files,
  and leave big files to a desktop host of the group.
- (b) A host can ask other members to host a file: a request that a member's device keeps and
  serves a given file for the group (it would need the member to accept, and a way for members
  to serve through or beside hosts, since members never dial members, U7).

**Alternatives kept for reference:**

**A: hosts keep small files and relay big ones on demand.** A host keeps a file of at most a cap
(100 MB was the guess) like a picture. A bigger file is not stored: when a member asks a host
for it, the host pulls the bytes on demand from whoever has them and is connected to it (the
author's sessions, announced holders, other member sessions, other hosts), verifies them
against the signed hash into a short-lived, size-bounded relay cache (10 min idle, one 2 GiB
file), and serves them on while the pull runs. Cost to the host: bandwidth (the file's size
twice per relayed download), not storage. Every big download needs a holder online at the
same time as a host. This was built first (commit b83819b3) and removed when B was chosen;
switching to A means lowering the constant and bringing the relay back.

**C: hosts never keep files, they only relay.** `HOST_KEEP_FILE_MAX = 0` plus the relay of A.
The lowest storage cost, but every download needs a holder online at the same time as a host,
so small files are often unavailable, which is worse than 1:1 for small attachments.

Acceptance tests (in `crates/core/tests/it/group_files.rs`):
- B: a file over the old A cap (101 MiB, sparse) is fetched and kept by the host by itself;
  members still wait for a tap; with the author offline, a member who taps gets identical bytes
  from the host and becomes a holder (`host_keeps_big_files_for_members`).
- B: a 24 MiB file is kept by the host while a member waits for a tap; it streams with progress;
  cancel makes it `Remote` again (`big_files_wait_for_a_tap_and_stream`).
- The largest file fits a host's group budget on its own (`group-runtime` unit test
  `the_largest_file_fits_a_hosts_group_budget`).

## 5. What the app sees

`GroupMessage.file: Option<GroupFile>` (with `text` = the caption):

```text
GroupFile { name, size, mime, state: GroupImageState, done: u64,
            kept_by_hosts: bool, holders: Vec<GroupFileHolder { did, name }> }
```

- `state` uses the picture states: `Ready` (here), `Loading` (on its way; `done` = verified
  bytes so far), `Remote` (tap to download), `Failed` (could not be fetched: "Not available
  right now. Try again later.", a tap tries again).
- `kept_by_hosts`: `size <= HOST_KEEP_FILE_MAX` (always true under option B).
- `holders`: who has a copy now (§6). The author comes first unless they deleted it.
  Example UI line: "Anoop is also hosting this file" or "Anoop and 2 others have this file".
- Events: `GroupEvents.on_group_image(group, message)` fires when the state changes, when
  progress moves (at most once per 500 ms per file), and when the holders change. The app reads
  the message again. No new callback was added, so the existing Kotlin, Swift and desktop
  implementations keep compiling.

Calls: `group_post_file`, `group_fetch_file`, `group_cancel_file`, `group_file_path` (decrypts
once into `group-files/<hash>/<name>` in the account directory and returns that path),
`group_save_file(dest)` (decrypts to a path the user picked), `group_delete_file_copy`.

## 6. Holders ("Anoop and 2 others have this file")

- When a download completes and verifies, a member device writes
  `FileHeld{post, day, hash, held: true}`. It writes `held: false` when it deletes its copy.
  These are ordinary signed group records. Hosts relay them, and every device derives the same
  list.
- A host keeping a file does not announce (that is `kept_by_hosts`). A host's own device announces only if its user downloaded the file for
  themselves.
- Derivation: for each `(author DID, device)`, the latest `FileHeld` naming the post and its
  hash (by `at`, then record id) says whether that device holds the file. A DID holds the file
  if any of its devices does. Only current members count: a member who left or was removed
  drops off the list at once. The post's author holds the file unless they announced
  `held: false`.
- **Privacy:** this tells the group who downloaded a file. It is in line with group read
  receipts, which the group already sees. Open (public) groups send no receipts
  (`public-group.md` §4), so devices there **do not announce** holdings either.
- Cost: one small record per download and per deletion, in the post's day doc.

## 7. Limits

| What | Value | Where |
| --- | --- | --- |
| Largest file | 2 GiB (`MAX_FILE`) | record check, every device |
| Name | 1 to 255 bytes, no path separators | record check |
| Host keeps | every file, up to 2 GiB (`HOST_KEEP_FILE_MAX`, option B) | `group_core::record` |
| Group budget (pictures, voice, kept files) | 2 GiB + the tags of one 2 GiB file, per group per host | `engine/images.rs` |
| Auto-download | the 1:1 size limit (default 10 MB) and the group mode | unchanged settings |

## 8. Older apps

An app built before this change cannot decode `Body::File` or `Body::FileHeld`. The group doc
rule refuses the whole batch, as for any undecodable record (U22). The session to that host is
then closed with `Refused` (`engine/sessions.rs`), and the app dials again and is refused again.
The old app **does not crash**. What it shows: the file post never appears, and nothing newer in
that **day doc** arrives either. Other days still sync on the next session (the meta doc goes
first, then the days newest first, each in its own batch), but the session keeps closing every
time it reaches that day. The group may also show as unreachable now and then. This is the same
behaviour voice posts brought in v0.2.4. It is pre-release, so there is no shim, and the fix is to
update the app. `FileHeld` lives only in the day docs of file posts, so it adds no new days of
breakage.

## 9. Not built / follow-ups

- UI on iOS (`docs/TODO.md`).
- A direct member-to-member fetch over an existing contact link (optimisation).
- The user's ideas (a) and (b) of §4, and options A/C if host storage becomes an issue.
- A per-host setting for what a host keeps (today one constant, the same on every device, so
  that members can show `kept_by_hosts`).
