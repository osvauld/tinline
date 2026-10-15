# Host-storage receipt provenance: selected scheme and remaining gates

Status: the user selected **host-signed storage attestations**, with current verified
hosting authority on first acceptance and previously accepted statements retained
as historical observations. The [crypto/persisted-state slice](host-storage-attestation-slice.md)
now includes atomic proof acceptance and explicitly negotiated own-device proof
forwarding. External group-host receipt acquisition remains unfinished; the wire
test generates the host statement in a fixture, then uses real authenticated
laptop/phone sessions. This is distinct from own-device cache Acks.

The expanded selected design for recipient delivery/read signatures, independent
shard-root agreement and receipt compaction is recorded in
[Message receipt auditability](message-receipt-auditability.md). Most of that
recipient/shard layer is not implemented.

## Current evidence

`GroupMsg::Ack { doc, vv }` is scoped by the live group session. EndpointAuth proves
DID/device/group endpoint identity; session admission established the host role.
`group_reader` calls `group_acked` for a host we dialled, not member cache replies.
`GroupStore::{put_ack,put_ack_day}` store only a VV indexed by group/document and
host DID/device. They do not store a host signature over that group/doc/VV.

Therefore an existing stored Ack cannot be copied through self/3 and called a
standalone host-verified receipt. Authenticating the laptop's forwarding frame
proves who reported it, not that the external host made that statement. Original
message signatures prove authorship, not remote storage. Own-cache `Sync` receipts
remain advisory and never enter the group host-Ack stores.

## Choice A: own-device-vouched report

Forward a scoped report that the currently authorized own device says it observed
an admitted host Ack. Bind group, document, host DID/device and covered VV to the
report; label the provenance explicitly. Trust in the personal laptop is required:
a phone cannot independently distinguish a real observation from a fabricated one.
No host transport change, but this must not be marketed as a host-signed proof.

## Choice B: portable host-signed storage statement (recommended)

The host signs a dedicated-domain canonical versioned statement binding at least:

- group and canonical meta/day document;
- host DID and host's group device ID;
- the actual persisted native-operation frontier;
- relevant authorization context if the eventual acceptance policy requires it.

The laptop relays the original statement unchanged. The phone verifies the host
signature and separately verifies group-scoped hosting authority from original
signed group metadata. A valid DID signature alone grants no hosting authority.
Endpoint addresses/calling endpoints need not appear in the statement.

The statement means the host attests it stored those native operations, not that
all members received/read them or attachment bytes are available. A dishonest host
can lie; a signature is not a remote disk audit. Shared account recovery secrets
still do not provide strong isolation/revocation of hostile sibling devices.

Compatibility needs an explicit versioned capability/transport mechanism: group/2
Hello/Welcome do not have a receipt capability field. Do not append unconditional
new frames, overload valid day names with markers, or silently change the ALPN.
Old hosts retain direct authenticated Ack semantics; their old unsigned Ack rows
must not be upgraded into fabricated signed proofs.

## Selected first-acceptance policy

A receipt's claimed historical metadata frontier does not cryptographically prove
it was minted before host revocation. Choosing between acceptance under current
verified host authority and historical/admission-as-of authority is a policy choice.
The existing UI's stored Ack rows are historical observations; do not silently
invalidate them or apply a new trust rule to legacy data.

User-selected policy for *new forwarded signed statements*: require current
verified hosting authority on first acceptance; missing metadata means pending,
not trusted. Previously accepted statements remain historical observations.
This can reject a legitimate late statement from a now-revoked host. It does not
change existing legacy Ack data or resolve provisional regrant/epoch policies.

The current `group_message` UI also labels nonlocal-writer rows Delivered without
consulting host Acks. Own-device-authored borrowed/history rows need a separate
status review; this audit does not modify UI integration or claim those labels are
already a full receipt-provenance presentation.

## Tests before enabling a forwarded proof

- Independent synthetic host DID, two own replicas: accept only a valid host-signed
  group/doc/device/frontier statement after real host persistence.
- Bind all fields/domain/version; reject substitution, forged host identity,
  foreign group/day, member/laptop signatures and unsupported versions.
- Reject/defer absent membership metadata, unapproved host device, removed/revoked
  host according to the selected policy; do not promote an account signature to role.
- Persistence/quota failure on host emits no stored statement; append/metadata
  failure on phone leaves owed obligations intact. Persist proof/provenance before
  indexing its VV or clearing covered debt. Stale/partial statements cannot clear
  newer accepted frontiers.
- Lost proof, lost cache Ack, reconnect, duplicate replay and clean reopen:
  retransmit safely; retain original host statement and author operation proofs.
- Legacy peers: no new frames without explicit support; no fabricated provenance.
- Keep local/laptop cache, host-stored, member-stored and read separate. Keep direct
  enabled until upstream receipt and scoped readiness/fallback gates are satisfied.

Related: [shared receipt auditability](message-receipt-auditability.md),
[limits reference](sync-receipt-limits.md),
[integration gates](laptop-forwarding-integration-gates.md),
[history wire slice](own-history-wire-slice.md).
