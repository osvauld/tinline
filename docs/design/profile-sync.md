# Account profiles, pictures, and owner consent

Worktree: `.claude/worktrees/profile-sync`, branch `profile-sync`, based on `dev`.
`design-refinement` owns profile-page/avatar UI; this branch supplies the core APIs.
UI requirements/task packet: [profile-ui-handoff.md](profile-ui-handoff.md).

**Latest user correction supersedes the earlier optional-deletion policy:** picture viewing
permission is conditional on honoring later owner deletion. Replacement/removal automatically
issues the signed previous-hash deletion request, with no opt-in checkbox or recipient opt-out.
All four public picture actions now implement this; the public boolean has been removed.
The earlier 285-test result verifies the old policy; the automatic-deletion run is recorded below.

## Agreed direction

- Every stable account DID has a profile, independent of its devices.
- A separate Loro document carries profile fields. Independent field edits merge independently.
- Fields are extensible JSON data, including nested values; unknown fields survive sync/restart.
- Initial contact sync includes the profile/picture; later updates arrive without a call or visible message, and catch up after reconnect.
- Own linked devices sync the profile, including availability, and cached contact profiles/pictures.
- A picture descriptor changes atomically: ciphertext hash, random encryption key, normalized-content hash, and ciphertext size. Clearing the reference restores initials.
- Replacing/removing a picture automatically issues a scoped, DID-signed deletion request for the previous hash. This is inherent in the owner's control over their picture, not separate opt-in consent. Conforming recipients must honor it when received, with current-picture protection.
- Recipients verify consent/signature/scope/owner and drop only that owner's cached-picture claim. No current picture, other owner's reference, or chat media is deleted by this permit. If concurrent edits select the target hash as current, retain/defer the permit instead of rejecting the merged profile; apply it once that hash is no longer selected.
- Deletion is cooperative, not guaranteed remote erasure. Copies/screenshots/backups and hostile recipients cannot be revoked. Historical metadata/keys/removed fields remain in Loro history, which is synchronized to authorized peers; field removal is not history erasure.

## Implementation: reuse, not a second sync system

The model lives in the `account-model` crate (`crates/account-model/src/`), re-exported by p2pcore under the old paths; p2pcore's `profile/` keeps the node API and picture transfer.

`profile.rs` (p2pcore's `profile::model`) contains the small picture descriptor and schema/bounds validation ONLY. The originally proposed signed-snapshot envelopes and signature-frontier layer were removed.

- `sync::Doc`: existing snapshot/version-vector/Loro primitives.
- `sync/profiledoc.rs`: `profile`, with JSON fields in `fields/<name>` and signed deletion permits in `deletions/<old hash>`.
- `sync/profilecache.rs`: private own-device `profile-cache`, holding cached contact snapshots. Inner Loro histories merge so an outer cache LWW field cannot roll a contact profile back. The cache is never sent to contacts.
- Existing `tinline/self/3`: advertises both documents and uses the existing signed `Hello`/`Sync`/empty-ack round trip. Cached profiles are vouched for by authenticated own devices, consistent with the shared-account trust model; ordinary profile fields do not have a second per-field signature scheme. Deletion permits retain the original owner's DID signature even when forwarded.
- Existing chat control: reserves `profile` in `Hello.shards`, `Sync.doc`, and `Ack.doc`. A contact update is bounded/validated in a scratch document before durable publication. The session's verified DID owns this document; existing device-batch signature, message-token, contact and block admission checks still apply.
- Chat ALPN is `tinline/chat/4`: the older day-only parser would reject the reserved document name, so `/3` is clearly refused with the existing version mechanism rather than silently breaking it. No new channel or bincode message variants. Existing data is not reset by this feature. All clients need the updated core for profiles/shared availability; older own-sync clients do not advertise these new docs.
- Existing encrypted `BlobHub`: uploads/downloads use the existing chunked AEAD and iroh-blobs transfer. Picture keys/descriptors and retention markers are sealed under the account store. No plaintext image cache is written by this core.
- Profile blobs have owner-specific `profile/<owner DID digest>/<ciphertext hash>` protection tags, independent of chat's `tl/<hash>` tags. Deletion cannot remove another owner's or chat's claim on shared ciphertext. GC reclaims bytes after the last claim goes (normally the next 30-second cycle).
- Per-owner/hash claim guards serialize verified readiness publication and cache cleanup; cleanup rechecks durable markers after acquiring the guard, so a stale deletion task cannot drop a newly ready current claim. Other owners/hashes remain independent.
- Hub startup reconciles profile tags against durable retention markers. Failed/uncommitted uploads and interrupted signed deletions are recoverable without dropping chat tags or retained pictures.
- Profile reference/readiness/automatic previous-hash deletion request/outbox are published in one account-store transaction after bytes are stored. A rejected/failed metadata write does not replace accepted state. Background tasks check the account epoch; old downloads cannot overwrite the selected descriptor.

## Owner-signed deletion permit

Uses existing `proto::sign_blob`/`open_blob`, not a new cryptographic primitive:

```
SignedBlob { payload: base64url(JSON), signature: base64url(Ed25519) }
JSON = { v:1, owner:DID, picture_hash:64-lowercase-hex,
         scope:"cached-profile-picture", consent:true }
domain = "tinline-profile-picture-deletion-v1\0"
```

An explicit `false` consent or other scope is invalid. Replay is idempotent; consent is hash-specific and retained for offline catch-up. A deletion request is not authority to edit a profile or delete a conversation. A new picture uses a fresh random key/ciphertext hash, so an old permit cannot target its bytes.

Delivery is to current unblocked contacts and own devices. There is no promise of delivering new permits to former/blocked contacts, or erasing data already copied elsewhere.

## API for the designer / app integrator

UniFFI exports:

- `account_profile(did) -> AccountProfile?`: `did`, `fields_json`, `picture_hash?`, `picture_ready`.
- `set_profile_field(key, json_value?)`: patch one field; `None` explicitly deletes an optional field. Unknown fields are preserved. Name and availability are required local settings; use picture APIs for `picture`.
- `set_name` / `set_available`: existing calls now write shared profile fields. A locked name edit is recorded for application on next unlock. Contact aliases remain separate from the contact owner's name.
- `set_profile_picture(bytes)` / `replace_profile_picture(bytes)`: automatically sign a deletion request for the previous selected hash, if changed.
- `clear_profile_picture()` / `remove_profile_picture()`: automatically sign a deletion request for the removed selected hash, if present.
- No public opt-out boolean. First upload, same normalized-picture selection, and repeated removal are no-ops with respect to deletion. Failed metadata writes publish neither an edit nor its request.
- `request_profile_picture_deletion(old_hash)` / `picture_deletion_permits(did) -> [SignedBlob JSON]`.
- `profile_picture(did) -> PNG bytes?`: reads/decrypts/validates the currently selected local picture, or `None` when missing/removed. This is a blocking Node call: fetch from an app worker, not inside a core callback.
- `set_profile_events(ProfileEvents)`: `on_profile_changed(did)` for metadata and picture-readiness changes. Dispatch to the app model/UI thread; re-read state, don't infer it from event order.

Standard fields: `name` (string), `status`/`about` (strings), `availability` (`{available:bool, until:unix-seconds?}`), `picture` (descriptor). Additional fields are data, not scripts, HTML execution, or automatic URL/resource fetching. All profile fields currently share the contact audience; field-specific visibility/public group profiles are not implemented.

Availability controls call admission on linked devices after sync, and expires at `until` using the existing clock behavior. Actual online presence remains ephemeral and is not a persisted profile assertion. A fresh linked/restored install does not author default name/availability writes over an existing account profile; the supplied local/ticket name is a display fallback until the profile arrives. P2P propagation is eventual, not instant while devices are partitioned.

## Initial operational bounds

These are explicit implementation bounds, not a claim of unlimited history/storage:

- 64 fields; keys <=256 bytes, no control characters; values <=16 KiB, combined JSON <=64 KiB.
- Name <=256 bytes, no control/bidi-override characters (reuses the existing proto display-name character policy); status/about <=4096 bytes.
- Profile snapshot/update <=1 MiB; private profile cache <=12 MiB and <=1024 entries.
- <=4096 retained deletion records (snapshot bounds also apply). No automatic history/checkpoint pruning is implemented; over-limit changes fail rather than silently forget consent/history.
- Input image <=2 MiB; PNG/JPEG/WebP; bounded decode <=4096 pixels per side / 64 MiB allocation. Normalize to PNG <=512 pixels per side / 1 MiB, stripping original metadata.
- Picture sources, in order: our own devices (they serve cached contact pictures), then the contact's devices; 30 s per source; retries back off 1..60 s and the chat kick loop restarts a stopped download every 30 s. Since 2026-10-29 an own-device session coming up, or a wake from an own device, cuts every waiting backoff short (`ChatRt::profile_retry_now`), and a source with an unusable address is skipped instead of ending the attempt.
- Release-visible failure line (2026-10-29, `profile/fetch_log.rs`): when every source failed, one `warn` `profile picture <hash prefix> of <short DID> not fetched: own <device prefix>: <class>; contact <device prefix>: <class>`, logged when the reason first appears or changes and at most every 10 min while it stays the same (PROVISIONAL); classes only (`cannot connect`, `timed out`, `stalled`, `transfer failed`, a fixed protocol text), never a network error's text. A later success after a logged failure logs `fetched from <source> after earlier failures` at info.
- At most four concurrent picture fetches; background retries do not retain the account mnemonic/signing identity. Downloads require exact authenticated descriptor size, ciphertext hash, successful decryption, normalized content hash, and bounded PNG validation. A transfer exceeding declared size is aborted by the shared fetch path.

## Tests and status

### Automatic-deletion correction

- Test-first red: `cargo test -p p2pcore --test profile_sync default_picture_actions_automatically_request_old_copy_deletion` via buildlock, exit 101; expected one signed request after default replacement, got zero. Log: `/tmp/profile-auto-delete-red.log`.
- Added default-action/idempotence/failed-removal/durable-permit checks; updated the real-node offline contact case to automatic replacement and removal with actual GC; extended the offline linked-device case to signed request catch-up and old-byte GC. Existing concurrent-current-picture and failed-replacement protections remain.
- Current regression command (from this worktree):
  `python3 /home/abe/p2p_chat/scripts/buildlock.py --who profile-sync -- cargo test -p proto -p p2pcore --lib --test profile_sync --test accounts --test vault --test restore --test link --test calls --test grant_characterization --test readd --no-fail-fast`
  **Result: 286 passed, 0 failed; `TEST_EXIT=0`.** Breakdown: 135 core unit, 65 proto unit, 9 real-node profile acceptance, 11 accounts, 12 calls, 8 grant-characterization, 28 QR-link, 4 re-add, 6 restore, 8 vault. Log: `/tmp/profile-auto-delete-green.log`. `git diff --check` passes.

### Earlier implementation history

- Initial six document tests passed before integration: `/tmp/profile-sync-green.log`.
- Integration API tests were written first; the red run failed on missing production APIs: `/tmp/profile-sync-integration-red.log`, exit 101.
- Targeted integrated run:
  `python3 /home/abe/p2p_chat/scripts/buildlock.py --who profile-sync -- cargo test -p proto -p p2pcore --lib --test profile_sync --no-fail-fast`
  passed **133 core unit tests + 7 real-node profile acceptance tests + 65 proto unit tests**, exit 0 (`/tmp/profile-sync-tests-v2.log`).
- Broad regression run passed **283 tests**, exit 0 (`/tmp/profile-sync-regressions.log`), and the concurrency-bound/upgrade-message verification passed **206 tests**, exit 0 (`/tmp/profile-sync-final.log`). A subsequent broad run including bidi-name protection also passed **283 tests**, exit 0 (`/tmp/profile-sync-final-regressions.log`).
- Additional test-first fixes: bidi-name acceptance test red (`/tmp/profile-sync-name-red.log`, exit 101); concurrent current-picture/old-consent merge red (`/tmp/profile-sync-consent-red.log`, exit 101). The latter showed why current-picture protection belongs in cache execution rather than rejecting a valid merged CRDT document. Local APIs still refuse issuing a request for the locally current picture.
- Final command (includes the added real-node concurrent-permit/GC case):
  `python3 /home/abe/p2p_chat/scripts/buildlock.py --who profile-sync -- cargo test -p proto -p p2pcore --lib --test profile_sync --test accounts --test vault --test restore --test link --test calls --test grant_characterization --test readd --no-fail-fast`
  Result: **285 passed, 0 failed; `TEST_EXIT=0`**. Breakdown: 135 core unit, 65 proto unit, 8 real-node profile acceptance, 11 accounts, 12 calls, 8 grant-characterization, 28 QR-link, 4 re-add, 6 restore, 8 vault. Log: `/tmp/profile-sync-complete.log`.

Automatic-deletion implementation/API cleanup is complete and verified by the 286-test regression run above. Ready for integration into `dev`. `git diff --check` passes. UI implementation and platform installation/smoke testing remain with the app/designer integration, not claimed by these Rust tests.

Acceptance cases cover real independent endpoints, including current-picture protection across a concurrent signed permit and an actual GC interval: initial/live profile and picture sync without messages; offline catch-up; replacement/removal; linked profile/availability/picture convergence; a fresh sibling receiving cached contact profile/picture with the contact owner offline; automatic signed deletion and physical GC; block-aware serving; local validation/write rejection leaving accepted state unchanged; lock/reopen.

Limits: no UI implementation or device installation, no Android/desktop UI smoke test yet, and no full workspace/video release suite claimed. The restored-device registry helpers in some tests do not substitute for QR linking tests; `--test link` separately exercises the existing real link flow. A signed deletion request is never proof of secure remote erasure.
