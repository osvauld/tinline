# Rotation acceptance checklist (retired placeholders)

These 14 acceptance tests were `#[ignore]` placeholders in `crates/core/tests/rotation_acceptance.rs`.
Each called `unimplemented!()` stubs for an R1 API that does not exist, so none could run if un-ignored.
They were moved here when the placeholders were retired; the full bodies and the stub `harness` module
are restorable from git tag `archive/pre-retire-2026-10-14`. Spec: `docs/design/rotation-open-decisions.md`
(U1, U2, U5 revised, U7, U8; section 3b for storage ordering).

Ignored tests with a real body: none. The one remaining test in the file,
`endpoint_is_stable_without_a_due_rotation`, runs today.

When the rotation feature is built, write each item below as a real test.

- [ ] `private_endpoint_is_what_contacts_dial_and_differs_from_the_group_endpoint`
  - Spec: U2 — two endpoints per device: the private one is what tickets and contacts dial; the group one is distinct and never appears in 1:1 state; same DID.
  - Blocked on: two endpoints per device (U2)
  - Cites: `docs/design/rotation-open-decisions.md` U2
- [ ] `group_traffic_never_carries_the_private_endpoint`
  - Spec: U2 + U7 — group traffic never reveals a member's private endpoint.
  - Blocked on: group chat API + two endpoints (U2, U7)
  - Cites: `docs/design/rotation-open-decisions.md` U2, U7
- [ ] `rotation_follows_the_configured_interval`
  - Spec: U1 (periodic, configurable rotation; Settings shows interval, last/next change) + U5 revised (nothing else triggers it).
  - Blocked on: rotation scheduler + clock hook
  - Cites: `docs/design/rotation-open-decisions.md` U1, U5
- [ ] `online_contact_switches_to_the_new_endpoint_without_user_action`
  - Spec: U1 + U8 — after a rotation an online contact receives the DID-signed note and keeps chatting and calling over the new endpoint without user action; Settings counts it as told; DID and writer ID unchanged.
  - Blocked on: forced rotation + rotation status
  - Cites: `docs/design/rotation-open-decisions.md` U1, U8
- [ ] `offline_contact_catches_up_when_it_comes_back`
  - Spec: U1 — a contact offline during the rotation catches up when it comes back (the side whose address is still live retries), before the old endpoint is retired or after.
  - Blocked on: forced rotation
  - Cites: `docs/design/rotation-open-decisions.md` U1
- [ ] `offline_contact_catches_up_when_the_rotating_side_comes_back`
  - Spec: U1 — the rotating side was the one offline: it rotated, could not tell the contact, went offline; when it comes back it tells the contact (whose address is still live).
  - Blocked on: forced rotation
  - Cites: `docs/design/rotation-open-decisions.md` U1
- [ ] `old_endpoint_stops_accepting_after_the_grace_window`
  - Spec: U1 — the superseded endpoint keeps accepting during the grace window, then stops.
  - Blocked on: forced rotation + grace window
  - Cites: `docs/design/rotation-open-decisions.md` U1
- [ ] `blocked_contact_does_not_learn_the_new_endpoint_and_loses_reach`
  - Spec: U5 revised + U1 + U8 — a blocked contact is excluded from the rotation note, never learns the new endpoint and loses reach once the old one is retired; the block itself did not rotate; "told N of M" counts unblocked contacts only.
  - Blocked on: visible block + forced rotation
  - Cites: `docs/design/rotation-open-decisions.md` U5, U1, U8
- [ ] `unblocked_contact_gets_the_current_endpoint`
  - Spec: U5 revised / 8b Q5 + U8 — on unblock the contact is sent the CURRENT private endpoint (rotated during the block) and can reach us again.
  - Blocked on: visible block + unblock + forced rotation
  - Cites: `docs/design/rotation-open-decisions.md` U5, 8b Q5, U8
- [ ] `linked_devices_rotate_independently_and_contacts_reach_both`
  - Spec: U2 + U8 — linked devices rotate independently; a contact holds the current private endpoint of each and reaches both (calls ring both, messages reach both).
  - Blocked on: forced rotation + device-list push (P4)
  - Cites: `docs/design/rotation-open-decisions.md` U2, U8
- [ ] `restart_mid_rotation_leaves_a_consistent_reachable_state`
  - Spec: U1 + U8 (storage ordering, section 3b) — a crash after any rotation step leaves one consistent state after restart: the current endpoint is either the old or the new one (never a third), the contact reaches it without user action, and DID/writer ID are unchanged.
  - Blocked on: forced rotation + crash injection
  - Cites: `docs/design/rotation-open-decisions.md` U1, U8, section 3b
- [ ] `both_rotated_apart_without_a_shared_group_reports_lost_contact`
  - Spec: U1 — both sides rotated while apart, no shared group: each reports "lost contact" (not a silent hang, not "offline").
  - Blocked on: forced rotation + lost-contact state
  - Cites: `docs/design/rotation-open-decisions.md` U1
- [ ] `readding_by_fresh_ticket_restores_the_same_contact_with_history`
  - Spec: U1 (manual re-share) — after "lost contact", scanning a fresh ticket/QR restores the SAME contact (one entry, same DID) with its history and verification, and both directions work.
  - Blocked on: forced rotation + lost-contact state
  - Cites: `docs/design/rotation-open-decisions.md` U1
- [ ] `both_rotated_apart_with_a_shared_group_recover_via_the_host`
  - Spec: U1 (group audience) — both sides rotated while apart but share a group: they recover through the group's host without a re-share and never report lost contact.
  - Blocked on: group chat API + forced rotation
  - Cites: `docs/design/rotation-open-decisions.md` U1
