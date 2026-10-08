# Multiple accounts and devices — discussion decisions

Status: account-storage foundation started; Node/UI integration, migration, pairing, sync and call fanout remain unimplemented.

## Implementation progress

- `crates/core/src/accounts.rs`: separate redb per DID; passphrase-wrapped random DEK; sealed account/device labels and data records; fresh per-enrollment device key; one active storage session with stale-session fences.
- Record encryption authenticates the DID and record key as associated data, preventing ciphertext substitution between records/accounts.
- This API currently requires a passphrase. Platform-key-only onboarding is deferred until key persistence has an acknowledged, recoverable transaction.
- `crates/core/tests/accounts.rs`: identity restore/device separation, ciphertext-only persistence checks, permissions, namespace validation, account isolation, stale sessions and tamper/substitution rejection.
- This foundation is not yet wired into existing Node storage. Existing plaintext persistence remains until migration/integration lands.
- Process death during new DB reservation can leave an incomplete account file; recovery UX/crash handling is still required before release. Account filenames and record names remain visible; sealed storage is not metadata anonymity.

## Agreed model

- Reuse/adapt osvauld2's separate database per account and sealed-record model.
- Many identities may be stored; only one identity is unlocked and online locally at a time.
- The same mnemonic produces the same identity DID on desktop and mobile.
- Each installation/account enrollment generates its own random transport device key.
- On create/import, ask for a device label (for example "Personal phone" or "Work desktop"). Labels are display metadata, not authentication.
- A contact is one DID with several authenticated callable device addresses.
- Calling a contact should ring its known devices concurrently. Select exactly one answer; cancel all other attempts and stop their ringing.
- Own-device pairing and sync are separate from contact invitation/call authorization.
- "You" is an identity-scoped saved-items conversation synchronized between paired devices.
- Once a contact connects to a known device, that device supplies authenticated information about the identity's other devices. This does not solve discovery when all known addresses are offline.
- Declining a call on one device declines the whole logical call and stops ringing on the others. Answer/decline races still need a deterministic coordinator.
- Chat and contact data synchronize between own devices through updates; a named Loro contact document is the proposed representation.
- Contact aliases, blocks and verification state synchronize between paired devices. Verification must be tied to an explicit policy/device-set version so stale updates cannot silently restore trust after device changes.
- Extensive automated tests are a release requirement for account isolation, sealed persistence, pairing, convergence and multi-device call coordination.
- Image previews are deferred to a separate session.

## Current implementation gaps

- Node dialing is sequential and returns the first connection, not the first accepted answer.
- Contacts retain at most four device keys and one contact-wide relay hint, without device labels.
- A restored mnemonic recovers identity but does not discover other devices or restore account data.
- Contact tickets carry signed invitations/address hints; mutual call grants are exchanged during the handshake. These do not authorize account sync.
- osvauld2 derives its device key from the mnemonic; do not carry that behavior into this app's per-install device model.

## Decisions still needed

Device linking (pairing, registry, sync scope, unlink) is proposed in `device-linking.md`.

1. Device registry: authenticated device labels, per-device relay hints, membership updates, and distribution to contacts.
2. Pairing: explicit approval, short-lived single-use challenge, same-DID proofs bound to transport keys, and sync-specific authorization.
3. Call race: shared call ID, answer selection/confirmation, simultaneous answers, cancellation retries/timeouts, and hangup during dialing. Only the selected device starts media.
4. Decline coordination: global decline is agreed; define delivery, acknowledgments and answer/decline race handling.
5. History: canceled siblings show "answered elsewhere", not missed; reconcile into one logical call record.
6. Sync scope/conflicts: contacts, grants, blocks, redeemed tickets, history, saved items and deletions. Decide account-wide versus device-local availability/settings. Never synchronize a device's transport secret or overwrite another device's local vault key.
7. Device removal: distinguish unlinking sync from revoking identity authority. Possession of the mnemonic permits minting new attestations under today's model; unlink is not global cryptographic revocation.
8. Offline behavior: initial sync is peer-to-peer when devices overlap online; define retry/resume and user-visible status. Offline/new addresses require a discovery/update path.
9. Account switching: reject or explicitly end an active call; fence stale jobs/callbacks from the previous account.
10. Migration/recovery: transactional encrypted migration, platform-key persistence failures, and safe handling of authentication/corruption errors without silently erasing state.

## Required test matrix (planned, not yet implemented)

- Accounts: separate DIDs/databases; switching drops prior secrets; stale workers/callbacks cannot read/write the next account; active-call switch policy.
- Identity/devices: mnemonic restore preserves DID but creates a different device key; local vault keys and transport secrets never synchronize.
- Sealed storage: no sensitive fixture text in persisted records/temp files; wrong keys, tampering and unsupported versions fail safely; migration crash boundaries and platform-key save failures remain recoverable. Ciphertext-only persistence tests do not prove secure deletion of historical plaintext.
- Pairing: same-DID success; wrong DID, transport-key mismatch, forged attestations, replayed/expired challenges, unapproved devices and call-grant-as-sync-token rejected.
- Device discovery: authenticated directory updates; forged/removed devices rejected under the chosen membership policy; offline known-address limitation surfaced.
- Loro sync: initial snapshot, incremental missing updates, duplicate/reordered updates, offline concurrent edits, reconnect/resume, schema validation and bounded hostile inputs.
- Contacts: aliases, blocks and deletions converge; concurrent removal/re-add policy tested; stale verification updates cannot undo a device-change reset.
- Saved/chat data: own-device convergence, history and deletions; unrelated identities/contacts cannot retrieve private account documents or blobs.
- Calls: multiple devices ring with one logical call ID; first selected answer wins; simultaneous answers yield exactly one media session; losing devices stop ringing without missed-call records.
- Global decline: all reachable devices stop; answer/decline race, dropped cancellation, retry, caller hangup, busy/offline sibling and timeout behavior tested.
- Call history: one logical record reconciles across devices, with answered-elsewhere state where appropriate; no duplicate missed notifications.
- End-to-end: desktop/desktop and desktop/Android pairing, restore, sync and call fanout on direct and relay paths; restart, lock/account switch, permission denial and network loss.

Use deterministic clocks and controlled delivery/failure injection for race/crash tests, plus randomized update schedules for convergence. Physical-device Android tests remain necessary alongside automated suites.

## Existing design boards to revise after decisions

- Flow.dc.html: account selection, import + device name, link device, initial sync, multi-device ringing.
- AndroidOnboarding.dc.html / AndroidUnlock.dc.html: device naming; import adds an account rather than blindly replacing it; recovery versus data sync distinction.
- AndroidSettings.dc.html / DesktopMore.dc.html: account switcher, linked devices, rename/unlink, sync status.
- AndroidContact.dc.html: one contact with several authenticated devices.
- AndroidCalls.dc.html / DesktopCalls.dc.html: answered-elsewhere state and multi-device decline semantics.
- AndroidChats.dc.html / DesktopChat.dc.html: "You" saved-items entry.

The HTML boards were copied from an external design canvas; coordinate canvas updates before treating local board edits as the final source of truth.
