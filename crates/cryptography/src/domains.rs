//! Every domain-separation label for signatures and hashes, in one place. Each is fed to
//! `sign::input` / `hash::hasher`, which length-prefix it, so no label needs a terminator and
//! none can be confused with a prefix of another; `ALL` is checked for duplicates.
//! HKDF contexts that derive identity keys (`osv/identity/*/v1`, `osv/ecies/v1`) are NOT here:
//! they are a forever contract of the DID and live with their derivation.

/// A domain-separation label.
pub type Domain = &'static [u8];

// ---- signatures: proto (DID or device key over JSON claims) ----
pub const ATTEST: Domain = b"osvauld/p2p/attest/v1\0";
pub const INVITE: Domain = b"osvauld/p2p/invite/v1\0";
pub const INVITE_V2: Domain = b"osvauld/p2p/invite/v2\0";
pub const GRANT: Domain = b"osvauld/p2p/grant/v1\0";
pub const BIND: Domain = b"osvauld/p2p/bind/v1\0";
pub const DEVICES: Domain = b"osvauld/p2p/devices/v1\0";
pub const BLOCK: Domain = b"osvauld/p2p/block/v1\0";
pub const GCALL: Domain = b"tinline/gcall/v1\0";
pub const PICTURE_DELETION: Domain = b"tinline-profile-picture-deletion-v1\0";

// ---- signatures: core ----
pub const CHAT_BATCH: Domain = b"tinline-chat-batch-v1";
pub const GROUP_RECORD: Domain = b"tinline-group-record-v1";
pub const GROUP_INVITE: Domain = b"tinline-group-invite-v1";
pub const GROUP_HOST_ADDR: Domain = b"tinline-group-host-addr-v1";
pub const GROUP_MAIL: Domain = b"tinline-group-mail-v1";
pub const GROUP_ENDPOINT: Domain = b"tinline-group-endpoint-v1";
pub const STORAGE_ATTESTATION: Domain = b"tinline-group-storage-attestation-v1\0";
pub const RECEIPT: Domain = b"tinline-recipient-receipt-v1\0";

// ---- hashes / ids ----
pub const GROUP_ID: Domain = b"tinline-group-id-v1";
pub const GROUP_RECORD_ID: Domain = b"tinline/group/record-id/v1";
pub const GROUP_CALLS_FRONTIER: Domain = b"tinline/group/calls-frontier/v1";
pub const ATTESTATION_DIGEST: Domain = b"tinline/group/attestation-digest/v1";
pub const WRITER_ID: Domain = b"tinline-writer-id-v1";
pub const LORO_PEER: Domain = b"tinline-loro-peer-v1";
pub const RECEIPT_MESSAGE: Domain = b"tinline-receipt-original-message-v1\0";
pub const RECEIPT_ID: Domain = b"tinline-recipient-receipt-id-v1\0";
pub const RECEIPT_DM_VERSION: Domain = b"tinline-receipt-dm-version-v1\0";
pub const RECEIPT_GROUP_VERSION: Domain = b"tinline-receipt-group-record-v1\0";
pub const RECEIPT_EVIDENCE: Domain = b"tinline-receipt-protocol-evidence-v1\0";
pub const CALL_JOINED: Domain = b"tinline-call-joined-transition-v1\0";
pub const GCALL_BOARD: Domain = b"tinline/gcall/board/v1";
pub const PICTURE_MARKER: Domain = b"tinline/profile/picture-marker/v1";
pub const PICTURE_CONTENT: Domain = b"tinline/profile/picture-content/v1";
pub const PROTECTED_BLOB: Domain = b"tinline/chat/protected-blob/v1";

// ---- keyed hashes (MACs) ----
pub const LINK_PROOF: Domain = b"tinline/link/proof/v1";
pub const LINK_CONFIRM: Domain = b"tinline/link/confirm/v1";
pub const LINK_TRANSCRIPT: Domain = b"tinline/link/v1";

/// Every domain above (checked for duplicates in tests).
pub const ALL: &[Domain] = &[
    ATTEST,
    INVITE,
    INVITE_V2,
    GRANT,
    BIND,
    DEVICES,
    BLOCK,
    GCALL,
    PICTURE_DELETION,
    CHAT_BATCH,
    GROUP_RECORD,
    GROUP_INVITE,
    GROUP_HOST_ADDR,
    GROUP_MAIL,
    GROUP_ENDPOINT,
    STORAGE_ATTESTATION,
    RECEIPT,
    GROUP_ID,
    GROUP_RECORD_ID,
    GROUP_CALLS_FRONTIER,
    ATTESTATION_DIGEST,
    WRITER_ID,
    LORO_PEER,
    RECEIPT_MESSAGE,
    RECEIPT_ID,
    RECEIPT_DM_VERSION,
    RECEIPT_GROUP_VERSION,
    RECEIPT_EVIDENCE,
    CALL_JOINED,
    GCALL_BOARD,
    PICTURE_MARKER,
    PICTURE_CONTENT,
    PROTECTED_BLOB,
    LINK_PROOF,
    LINK_CONFIRM,
    LINK_TRANSCRIPT,
];
