# Message receipt auditability: groups and individual chats

Status: **selected product direction; mostly not implemented**. This document
records the user decisions for portable receipt evidence, shared Loro machinery,
independent shard agreement and eventual receipt compaction. It does not settle a
new wire schema, shard-finalization authority or retention policy.

## Selected semantics

- Delivery to **one admitted device of a recipient account** is sufficient to
  count that account/member as delivered. It does not mean every linked device
  received the message.
- Delivered and read are **separate recipient-signed statements**.
- Individual and group call logs are also receipt-backed audit history, with typed
  session observations rather than message delivered/read claims. The
  [core/UI call contract](call-receipt-core-ui-contract.md) distinguishes local
  participation, host evidence and participant confirmations; another session
  owns UI implementation.
- Host storage, recipient delivery, recipient read and own-device/laptop cache
  observations are distinct evidence. A host signature cannot establish delivery
  or reading by another member.
- Retain original signed receipt evidence on individuals and authorized hosts;
  relays forward it unchanged. Derived status/index rows are not portable proof.
- Apply the same receipt primitives to groups and individual chats, while keeping
  their admission, privacy and replication policies separate.
- Later, recipients can independently reconstruct a shard and sign matching
  content commitments. They need not receive a shard file from the sender/host.
- Compact recipient-signed shard confirmations can supersede covered fine-grained
  receipts, with safe persistence/replication/archive gates before removal.

A signature is independently verifiable evidence of the signer's claim. It is
not a physical disk audit, proof that a human actually looked at a screen, or
strong revocation of a hostile device sharing the account recovery secret.

## What exists versus what is planned

Implemented in this audit worktree:

- Native Loro group operation transfer preserving original author/writer proofs.
- Host-signed storage statement codec and verification against persisted group
  metadata/current hosting authority.
- Atomic sealed-store acceptance of original host statements, host-VV projection
  and clearing only covered durable upstream obligations.
- Explicitly negotiated self/3 proof pagination between own devices. A tested
  authenticated laptop-to-phone session forwards an independent host's original
  signed proof unchanged. The host statement is generated in the fixture; this
  is **not** an authenticated three-node host-network acquisition test.
- Receipt application-message loss/retry, duplicate/reconnect, revoked own-device
  rejection and clean sealed-store reopen coverage. The latest complete owned
  regression passed **96 tests** before the shared primitive extraction; commands
  and limitations are recorded in the [host slice validation](host-storage-attestation-slice.md).
- Shared delivered/read codec and dedicated native Loro receipt map primitives:
  initial **13 new tests**, followed by typed call claims and transactional ledger
  primitives with **11 additional cases**; latest owned regression **127 passed**.
  These are not runtime confirmation or network issuance; see the
  [call/ledger slice](call-receipt-ledger-slice.md).

Tested primitives, not connected to runtime yet:

- Shared message delivered/read prototype codec and dedicated native Loro receipt
  map in `receipts{.rs,/doc.rs}`. It retains original signed exact hash batches,
  uses shared native insert inspection and whole-operation export planning, and
  requires a caller admission callback. Runtime coverage/admission, durable
  issuance and transport/status integration are not implemented. A low-level
  append-native-updates sealed ledger now commits original operations, VV and tiny
  indexes atomically. Real source/read/lifecycle derivation and historical authority
  provenance are still runtime-controller work. See latest 127-case validation.

Not implemented:

- Runtime member/chat receipt issuance, admission, UI provenance or wire
  negotiation; existing unsigned transport Acks are not retroactively signatures.
- Runtime typed call receipt issuance/storage hooks/sync/UI, despite tested payload
  and ledger primitives. See the separate proposed core/UI contract; current live
  call history remains legacy local JSON/view evidence.
- Canonical shard manifests/roots, recipient-signed shard summaries, archive/deletion
  or receipt document rollover.
- Negotiated external group-host receipt acquisition; route readiness/suppression.

The current runtime host-proof ledger is sealed KV storage, **not a Loro receipt
document**. The new recipient Loro primitive is a separate unfinished prototype,
not a migration of that ledger or tested runtime receipt exchange. Refactor/chat-version integration
remains deferred.

## Signed receipt contents

A common versioned, domain-separated envelope should bind at least:

- receipt kind: host-stored, recipient-delivered or recipient-read;
- exact conversation/group and scope;
- issuer account DID and scoped installation/device ID;
- exact covered original message/version hashes, or a validated compact coverage
  representation, or an exact shard-manifest/content root;
- authorization context needed for that conversation's admission check.

Hash the canonical **original signed message/version**, not rendered text,
re-authored relay content or arbitrary JSON. Verify existing hash definitions
before reusing one; random message IDs alone are not content commitments. Edits
and deletes must not accidentally inherit a receipt for a different version.

Do not sign a naked message hash: that would omit conversation, receipt kind and
issuer/device context. Canonical encoding/domain separation prevents the same
signature being interpreted as a different statement.

Delivered issuance follows durable local verification/storage. Read issuance
covers messages actually marked read under the eventual UI policy, not all
messages downloaded. Neither says attachment bytes were downloaded or retained;
attachment-content receipts would need an explicit separate claim.

Admission remains independent of the signature. For group member receipts use
member/device admission, **not host permission**. Direct chats use their own
verified identity/conversation rules. First-acceptance/removal/regrant handling for
these new recipient receipts needs a documented policy; the selected current-host
rule does not automatically settle it. Accepted historical evidence must not be
silently erased or relabeled after removal.

## Loro reuse and light fine-grained receipts

Use a dedicated receipt document namespace per conversation/shard period, reusing
existing persistence, native bounded operation export/import and negotiated sync.
The exact namespace and lifecycle are not assigned yet. Avoid adding receipt
operations to the message coverage being acknowledged: otherwise receipt traffic
can generate acknowledgements of acknowledgements and disturb message frontiers.

Loro supplies replication/convergence, not issuer authentication or permission.
Each receipt value remains original immutable signed bytes. Import validation
checks signatures, scope, issuer admission and coverage. Durable acceptance comes
before status projection or forwarding; transaction/retry behavior must not
fabricate confirmation after a persistence failure.

Reduce overhead by:

1. Coalescing multiple confirmations into one signature rather than one per
   message. Delivery can use validated original-writer/counter coverage where its
   exact message mapping is available.
2. Keeping read coverage exact. Sparse message hashes/ranges preserve unread
   holes; a downloaded Loro VV is not a read frontier.
3. Content-addressing/deduplicating identical original signed batches.
4. Deriving account-level status from at least one accepted device receipt,
   without issuing another fabricated account-wide signature.

Retain the issuing recipient's originals and the sender's received evidence.
Authorized group hosts retain group receipt evidence; individuals retain the
receipt evidence replicated to them for their conversations. Whether all group
members receive everybody's read evidence is a visibility/product decision, not
an implicit grant from hosting. Private-chat evidence is replicated only to its
participants and explicitly authorized personal replicas, not public group hosts.

No immutable signed receipt can be enlarged in place. Additional coverage means a
new statement/signature. Batching reduces growth, but total evidence still grows
with recipients, updates and time. Timers/budgets are provisional and indexed in
[the central limits reference](sync-receipt-limits.md).

## Independently constructed shard agreement

The intended process is **not proof of a particular shard-file transfer**:

1. Define a canonical shard manifest/coverage snapshot for the conversation and
   period. It names exact original message/version records and a root algorithm.
2. Sender, host and recipient independently reconstruct that declared content
   from their own verified persisted messages, regardless of delivery path.
3. Each computes the root locally. Never sign a root merely advertised by a peer.
4. The recipient signs its own delivered statement binding that manifest/root,
   shard ID/revision, conversation and issuer DID/device.
5. Verify the signature/admission and compare roots against locally reconstructed
   covered content before projecting receipt coverage.

Matching roots commit to the **same canonical declared contents**, assuming the
hash's collision resistance. They do not prove identical Loro oplogs, physical
export bytes, absence of unknown future messages, or how the content arrived.
A VV can describe native coverage alongside a content root, but arbitrary Loro
export-byte hashes are not canonical logical-content identities.

A sorted canonical digest list or a domain-separated Merkle construction are
candidate root formats; the exact scheme is not chosen. Merkle membership proofs
can show that an original message/version belongs to a signed shard commitment.
A root covers only the declared snapshot. A calendar day passing does not by
itself seal a shard: offline late messages, edits and deletes may require a new
revision. Finalization/revision authority and acceptance tests must be selected
explicitly before implementation.

Read is separate: an entire-shard read signature is permitted only when every
covered message/version qualifies as read. Otherwise retain sparse read receipts
or sign an exact read subset. Receiving or reconstructing a shard is not reading it.

## Supersession, archive and eventual removal

A recipient's compact signed shard-delivered statement can replace that
recipient's earlier delivered receipts only for exact covered messages/versions.
The same holds separately for fully covered read evidence. A host's shard
signature can replace **its own** storage statements, never other recipients'
statements. Missing recipient confirmations remain missing.

Before removing superseded evidence:

1. Verify the original replacement signatures, authorization/acceptance provenance
   and exact coverage. Preserve historical acceptance context needed for audit.
2. Persist replacement statements, manifest/root and enough original content or
   membership-proof material to audit message coverage after fine-grained receipts
   are removed. A retained root alone is not a message-membership proof.
3. Replicate replacements to the required authorized holders and establish durable
   catch-up. Required holders/acknowledgement and retention grace policy are open;
   neither EOF nor inventory equality proves durable acceptance.
4. Archive fine-grained receipts first; exercise recovery and audit from replacement
   evidence before enabling an explicit deletion policy.
5. Reclaim storage via compatible receipt-document rollover/compaction. Deleting
   Loro map entries alone may retain their operations and not reclaim oplog space.
   Old/offline replicas must not resurrect removed evidence or lose necessary sync
   predecessors; retirement/bootstrap compatibility needs tests.

A host-signed archive hash/Merkle root over other people's receipts **does not**
replace their signatures. If original leaves/signatures are erased and no valid
recipient-signed superseding summary is retained, independent recipient evidence
has been lost. Storing an archive elsewhere can save hot-device space only while
its availability/security/retention assumptions remain explicit.

## Acceptance tests before shipping

- Cross-conversation/kind/device substitution, forged issuer, altered message
  version, wrong root and foreign shard are rejected.
- One admitted device confirms its account; other devices remain individually
  unconfirmed. Host/laptop receipts never become recipient-delivered/read.
- Failure before durable message/receipt commit produces no confirmation/status;
  replay/reconnect/duplicates preserve identical original signed evidence.
- Independently received messages in different orders/export layouts produce the
  same declared canonical root; missing/extra/edited records do not.
- Read holes remain holes; downloaded history cannot advance read state.
- Late shard revisions do not inherit uncovered receipt claims. Replacement
  summaries retain auditable membership and historical authorization evidence.
- Archive/reopen/rollover with offline and legacy peers does not lose signatures,
  revive deleted receipt history or fabricate readiness. Measure actual disk/oplog
  reclamation, message-by-recipient growth, CPU and device energy.

Related: [typed call/ledger implementation](call-receipt-ledger-slice.md),
[shared primitive implementation](shared-recipient-receipt-slice.md),
[call receipt core/UI handoff](call-receipt-core-ui-contract.md),
[host provenance](host-storage-receipt-decision.md),
[implemented host slice](host-storage-attestation-slice.md),
[limits](sync-receipt-limits.md), [chat design](../chat.md).
