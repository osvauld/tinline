# UI task packet: account profiles and synchronized pictures

For the `design-refinement` coordinating agent. Core implementation was developed in the
`profile-sync` worktree for integration into `dev`. Update your branch from `dev` through your
normal coordinated integration workflow before binding these APIs; an older design checkout
may not contain them. Do not merge/stage another agent's uncommitted work.

Read the backend contract first:
`/home/abe/p2p_chat/.claude/worktrees/profile-sync/docs/design/profile-sync.md`.
Then follow your playground `AGENTS.md` and `PLAN.md`, including shared-file ownership,
one store, both form factors/themes, simulation, accessibility and QA.

## Goal

Design and simulate one profile per account DID, not per device. People can view profiles,
edit their own account profile, and choose a picture. Profile/picture changes should be
reflected consistently wherever that person appears. Initial contact sync, later edits,
reconnect catch-up and linked-device updates work without a visible chat message or call.

This packet defines behavior, not a prescribed visual composition. Use the existing Tinline
visual language; do not build an isolated profile mockup with its own store or shell.

## Required screens and entry points

1. **My profile:** an obvious entry from the account/settings area. Show picture/initials,
   name, status/about, account availability, and additional profile information. Edit the
   active account only. Keep device names/settings separate.
2. **Person profile:** reachable from a contact/person row and conversation header/avatar.
   Show the owner's profile read-only. Keep local contact alias, verification and contact
   actions visibly separate: changing my alias for someone does not rename their account.
   Retain existing block/call-permission rules; picture changes do not reset verification.
3. **Picture flow:** choose a file/photo, preview it, replace it, or remove it. Crop/reposition
   interaction is welcome if it fits the design, but is app-side preparation rather than a
   new core feature. Handle picker cancellation without changing the profile.
4. **Additional information:** support extensible profile data, not a permanently closed
   name-and-bio-only form. Provide a coherent add/edit/remove information pattern. Known
   fields get appropriate controls; extra fields should be readable, with safe presentation
   for structured values. A developer-style JSON dump must not be the primary profile page.

Do not add a public profile directory, per-field audience controls, group-wide profile
broadcast, or executable/custom-HTML profile widgets. Those are not backend capabilities.
For a group participant who has no accessible profile, retain the existing fallback; do not
invent discovery/sharing. Viewing their profile must not automatically add them as a contact.

## Shared avatar behavior

Coordinate with the playground lead before changing existing `lib/ui/Avatar.svelte`, shared
model/store/seed files, shells, or other agents' screens. Reuse/extend the shared avatar path;
do not implement a second independent avatar component in every screen.

Apply person pictures consistently to applicable people/chat lists, conversation headers,
profile views, call surfaces and group-member surfaces where an accessible person profile
exists. Group-owned avatars are a separate feature; do not replace group icons with a person's
picture or conflate group settings with account settings.

- No picture reference: initials using the existing style.
- Selected picture not yet cached: initials/placeholder with an unobtrusive loading/awaiting
  state. Missing/offline is not automatically an error. Do not invent percentage progress.
- Cached picture: render bytes supplied by the app adapter, never a guessed network URL.
- Picture reference changes: stop presenting the old image as the selected picture; ignore
  stale completions and load the new selection.
- Removal: clear the old image immediately after the removal is accepted and show initials.
- Broken/unsupported data: usable fallback, no broken-image glyph or stuck blocking spinner.

Keep stable keyed rows and shared identity lookups; don't wipe/rebuild lists on a profile edit.

## Picture ownership and automatic old-copy deletion

**User correction: deletion is NOT opt-in.** This picture belongs to the account owner.
Permission to view it is conditional on honoring the owner's later deletion request.
Replacing/removing it automatically issues the owner-signed request for the previous hash;
conforming clients must honor it when received, subject to current-picture protection.

- No consent checkbox, “keep old copies” switch, or recipient choice to ignore the request.
- The owner chooses Replace or Remove; old-cache deletion is inherent in that operation.
- Bind the signed request to the specific previous hash. Never erase unrelated pictures,
  another owner's references, or chat-media copies.
- Current pictures remain protected. A request targeting a hash selected by a concurrent edit
  is deferred until that hash is no longer current, rather than blocking profile convergence.
- Do not say “deleted everywhere”, “securely erased”, or claim completion across recipients.
- Do not expose keys, cryptographic descriptors, or signature blobs in normal profile UI.
- This does not introduce deletion/rotation/group-membership side effects into Block or unlink.

Non-optional behavior for conforming clients is distinct from technical enforceability.
Help text can say:

> Replacing or removing your picture requests deletion of the previous cached copy on your
> linked devices and contacts when they sync. Screenshots, backups, and copies retained by
> modified clients cannot be erased by the app.

A separate old-picture history browser is not required; the backend does not supply a history
browser/list API. Do not fake one as a shipped feature.

## Editing, save and privacy behavior

- Read-only person profiles; only my profile fields are editable as account-owned data.
- Patch edited fields individually; never replace the whole object with just fields your form
  understands. Preserve unknown keys and nested data. Optional-field removal is explicit.
- Form drafts/previews can be transient component state; accepted profiles belong in the shared
  store. On failed writes, preserve the last accepted name/picture/data and offer clear feedback.
- A locally accepted save can say “Saved”. Remote propagation is eventual; say it will sync when
  connected, not “everyone updated”. There is no per-contact profile-delivery status API.
- Availability is an account-wide setting synchronized across linked devices and affects call
  admission. Respect deadline expiry using the shared simulation clock. Actual online presence
  remains ephemeral and separate; don't label saved availability as proof someone is online.
- Profile fields share the contact audience; private/public toggles would be misleading.
- Removing a field hides it from the current profile, not from all historical replicas. Include
  appropriate privacy/help copy: previous versions can remain in synchronized history.
- Profile fields are data, not HTML/scripts. Never automatically fetch resources specified by
  arbitrary fields. Links, if supported, need deliberate safe navigation, not embedded execution.
- Never prepopulate profile data with recovery phrases, passphrases, endpoint addresses or keys.
- No anonymity promise; unlinking is not strong revocation of a recovery-phrase holder.

## App adapter / backend contract

Do not call Rust from the playground directly; simulate these contracts in the shared model.
Production integration can later bind them through the platform adapter.

| Need | Core API |
|---|---|
| Read profile | `account_profile(did)` -> optional `{did, fields_json, picture_hash?, picture_ready}` |
| Patch data | `set_profile_field(key, json_value?)`; `None` removes an optional field |
| Name / availability | existing `set_name`, `set_available` |
| Set/replace picture, automatic old-copy request | `set_profile_picture(bytes)` / `replace_profile_picture(bytes)` |
| Remove picture, automatic old-copy request | `clear_profile_picture()` / `remove_profile_picture()` |
| Read current image | `profile_picture(did)` -> optional PNG bytes |
| Changes | `ProfileEvents.on_profile_changed(did)`; reread metadata/readiness |

**Backend policy updated:** all four picture actions automatically sign a previous-hash
request when a picture is replaced/removed. There is no public `delete_old` boolean. Initial
upload, selecting the same normalized picture, and removing an already absent picture do not
create spurious requests. Failed saves publish neither the edit nor its deletion request.
The signed permit authorizes deletion; do not claim it is a new cryptographic read-access
permit or a secure remote-revocation scheme. See the backend contract for test results.

Standard fields: `name`, `status`, `about`, `availability:{available,until?}` and internal
`picture` descriptor. The avatar UI uses `picture_hash`/`picture_ready` and image bytes, not
editing the descriptor as an additional user field.

Production image reads are blocking Node calls: dispatch from an app worker, not UI/core
callback threads. Route events through the app model. Key asynchronous image results by
**active account DID + profile owner DID + picture hash** and discard results if any change.
Clear account-scoped image state/object URLs on account switch or lock; release obsolete URLs.

Initial limits: input PNG/JPEG/WebP <=2 MiB, dimensions <=4096 per side; core normalizes to PNG
<=512 per side and <=1 MiB. Explain unsupported/oversized input instead of silently accepting it.
Name <=256 UTF-8 bytes, no control/bidi-override characters; status/about <=4096 bytes;
64 fields, each JSON value <=16 KiB and combined fields <=64 KiB. Avoid character counters
that incorrectly promise the same limit as UTF-8 bytes. Preserve values on validation errors.

## Simulation and acceptance requirements

Add deterministic controls/scenarios using the existing drawer, shared store and `clock.now`:

1. New contact initially has initials; metadata arrives, then its picture becomes ready without
   any chat message or call. No unread badge or messaging notification is generated by this.
2. A contact changes name/status/picture while their profile and conversation are open; all
   applicable avatar/name surfaces update consistently while local aliases remain intact.
3. Old image load completes after a newer hash is selected: it cannot overwrite the newer view.
4. Remove picture -> initials. Every accepted replacement/removal automatically creates the
   specific previous-hash deletion request; no checkbox and no “remote erasure complete” state.
5. Offline edit -> local save -> reconnect catch-up. Another linked device edits a different
   field concurrently; both edits survive. Unknown/nested fields survive editing known fields.
6. Valid old-picture consent and a concurrent selected-picture conflict converge while the
   selected current picture remains intact; deletion remains deferred.
7. Shared availability changes from a sibling and expires at the deadline; online presence is
   separately simulated. A new linked install must not overwrite existing account settings.
8. Picture unavailable/offline, invalid file, oversized file and failed save remain usable;
   a failed replacement does not destroy the last accepted picture or create a deletion request.
9. Switch accounts or lock during loading: no old-account image/fields flash in the new view.
10. Blocking stops new profile updates/authorized downloads according to existing policy; do
    not present a new deletion/rotation/group-membership effect as part of Block.

Check both Android and desktop, light/dark, small widths, keyboard operation, logical CSS/RTL,
44px targets, screen-reader labels and reduced motion. No page-local animation/sound timing.
Run `npm run check` with 0 errors/warnings and relevant Playwright flows, inspect fresh-named
screenshots, and check console errors as required by the playground rules.

## Ownership and final report

The design-refinement lead should assign the profile area and coordinate avatar/store/route
changes with active area agents. This packet does not authorize overwriting their work or adding
an isolated store. Do not touch/stage/commit the profile-sync worktree.

Report: implemented routes/flows, shared-file requests or coordinated changes, simulations,
commands/results, screenshot paths, backend adapter requirements, and genuinely unresolved
product decisions. Proposed features belong in Simulation notes, not shipped-looking app copy.
