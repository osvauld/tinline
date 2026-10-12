//! Group records: every group op (genesis, join, post, admin grant, flag, host permit, ...) is
//! one DID-signed record. Nothing in a record is a network key (U4 revised): the author is a DID,
//! the device is the per-install random device ID (`dev`, U16), and the record's Loro peer id
//! must be `writer_id(author, dev)`.
//!
//! A record is stored as `SignedRecord` bytes; its id is the hex BLAKE3 of those bytes, so the
//! id binds the content and the signature. Records are immutable: an edit would be a new record.
//!
//! Outside the doc (never in a record): invites (`SignedInvite`, only its signed grant goes into
//! a `Join`), host addresses (`SignedHostAddr`) and the per-connection endpoint proof
//! (`EndpointAuth`). Those carry group endpoints and travel only member <-> host (U7).

use serde::{Deserialize, Serialize};

pub const GROUP_ID_DOMAIN: &[u8] = b"tinline-group-id-v1";
const RECORD_DOMAIN: &[u8] = b"tinline-group-record-v1";
const INVITE_DOMAIN: &[u8] = b"tinline-group-invite-v1";
const HOST_ADDR_DOMAIN: &[u8] = b"tinline-group-host-addr-v1";
const AUTH_DOMAIN: &[u8] = b"tinline-group-endpoint-v1";

/// Largest stored record (a post's text is capped well below).
pub const MAX_RECORD: usize = 64 * 1024;
pub const MAX_TEXT: usize = 20_000;
pub const MAX_NAME: usize = 100;
/// A `seen` frontier with more writers than this is refused (bounds a record's size).
pub const MAX_SEEN: usize = 1024;

/// A group's id: hex BLAKE3 over a domain, the owner's DID and the genesis nonce.
pub fn group_id(owner: &str, nonce: &[u8; 16]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(GROUP_ID_DOMAIN);
    h.update(&(owner.len() as u32).to_be_bytes());
    h.update(owner.as_bytes());
    h.update(nonce);
    h.finalize().to_hex().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Body {
    /// The group exists: written once by the owner; `group_id(author, nonce)` is the group.
    Genesis { name: String, nonce: [u8; 16], owner_name: String },
    /// The author joins with an invite issued by the owner or an admin; `name` is the display
    /// name the author shows the group (members need not be contacts).
    Join { invite: SignedInvite, name: String },
    /// The author leaves.
    Leave,
    Post { text: String, reply_to: Option<String> },
    /// Owner only: `member` is (or stops being) an admin.
    Admin { member: String, admin: bool },
    /// Admin: one flag towards removing `member` (U9/U15: any 3 flags remove).
    Flag { member: String },
    /// Owner or admin: device `host_dev` of `host` may (or may no longer) host the group.
    HostPermit { host: String, host_dev: [u8; 32], active: bool },
    /// The author's own device `dev` agrees to host (or stops hosting). Hosting is opt-in per
    /// device: a host is active only with both a permit and this.
    HostAccept { active: bool },
    /// The author's encryption public key (`identity::Identity::encryption_public_key`, derived
    /// from the recovery phrase; not a network key), so other members can seal mail to it (U3
    /// chat requests, `mail.rs`). The latest per author counts.
    EncKey { key: [u8; 32] },
    /// Owner or admin: device `host_dev` of `host` may (or may no longer) host group CALLS
    /// (U21: a separate `host_call` capability, never implied by `HostPermit`). Latest by
    /// `(author clock, id)` per device wins, like `HostPermit`.
    CallHostPermit { host: String, host_dev: [u8; 32], active: bool },
    /// The author's own device `dev` agrees to host group calls (or stops). Separate from
    /// `HostAccept`: agreeing to relay chat is not agreeing to carry calls (battery, uplink, the
    /// group endpoint shown to every caller), so a call host needs both a `CallHostPermit` and
    /// this.
    CallHostAccept { active: bool },
}

/// One group op, as signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub v: u8,
    pub group: String,
    pub author: String,
    /// The writing install's device ID (U16). `writer_id(author, dev)` is the record's Loro peer.
    pub dev: [u8; 32],
    /// Unix ms, the author's clock (display order only).
    pub at: i64,
    /// The signed frontier (U11): the author's version vector when writing, `(peer, counter)`
    /// sorted by peer. Decides "before or after a removal" in every delivery order.
    pub seen: Vec<(u64, i32)>,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedRecord {
    /// bincode of `Record`.
    pub body: Vec<u8>,
    pub sig: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    #[error("record does not decode")]
    Undecodable,
    #[error("record too large or malformed")]
    Malformed,
    #[error("record is for another group")]
    WrongGroup,
    #[error("signature does not verify")]
    BadSignature,
}

fn put_bytes(v: &mut Vec<u8>, b: &[u8]) {
    v.extend_from_slice(&(b.len() as u32).to_be_bytes());
    v.extend_from_slice(b);
}

fn sig_input(domain: &[u8], body: &[u8]) -> Vec<u8> {
    let mut v = domain.to_vec();
    put_bytes(&mut v, body);
    v
}

pub(crate) fn put_bytes_pub(v: &mut Vec<u8>, b: &[u8]) {
    put_bytes(v, b)
}

pub(crate) fn verify_did_pub(did: &str, msg: &[u8], sig: &[u8]) -> bool {
    verify_did(did, msg, sig)
}

fn verify_did(did: &str, msg: &[u8], sig: &[u8]) -> bool {
    let Some(key) = identity::public_key_from_did(did) else { return false };
    let Ok(sig) = <[u8; 64]>::try_from(sig) else { return false };
    identity::verify(&key, msg, &sig)
}

impl Record {
    fn check_shape(&self) -> Result<(), RecordError> {
        let bad = Err(RecordError::Malformed);
        if self.v != 1 || self.author.is_empty() || self.author.len() > 200 || self.seen.len() > MAX_SEEN {
            return bad;
        }
        if self.seen.windows(2).any(|w| w[0].0 >= w[1].0) {
            return bad;
        }
        match &self.body {
            Body::Genesis { name, owner_name, .. } if name.len() > MAX_NAME || name.trim().is_empty() || owner_name.len() > MAX_NAME => bad,
            Body::Join { name, .. } if name.len() > MAX_NAME => bad,
            Body::Post { text, reply_to } if text.len() > MAX_TEXT || text.is_empty() || reply_to.as_ref().is_some_and(|r| r.len() > 64) => bad,
            Body::Admin { member, .. } | Body::Flag { member } if member.len() > 200 => bad,
            Body::HostPermit { host, .. } | Body::CallHostPermit { host, .. } if host.len() > 200 => bad,
            _ => Ok(()),
        }
    }

    /// Signs as `account` (whose DID must be `self.author`).
    pub fn sign(&self, account: &identity::Identity) -> SignedRecord {
        debug_assert_eq!(account.did(), self.author);
        let body = bincode::serialize(self).expect("record serialises");
        let sig = account.sign(&sig_input(RECORD_DOMAIN, &body)).to_vec();
        SignedRecord { body, sig }
    }
}

impl SignedRecord {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("record serialises")
    }

    /// Decodes stored bytes and verifies them: shape, group, and the author's DID signature.
    pub fn open(bytes: &[u8], group: &str) -> Result<Record, RecordError> {
        if bytes.len() > MAX_RECORD {
            return Err(RecordError::Malformed);
        }
        let s: SignedRecord = bincode::deserialize(bytes).map_err(|_| RecordError::Undecodable)?;
        // Canonical: the stored bytes are exactly what re-encoding gives (no trailing junk, so
        // two byte strings never carry one record under two ids).
        if s.to_bytes() != bytes {
            return Err(RecordError::Undecodable);
        }
        let rec: Record = bincode::deserialize(&s.body).map_err(|_| RecordError::Undecodable)?;
        if bincode::serialize(&rec).ok().as_deref() != Some(s.body.as_slice()) {
            return Err(RecordError::Undecodable);
        }
        rec.check_shape()?;
        if rec.group != group {
            return Err(RecordError::WrongGroup);
        }
        if !verify_did(&rec.author, &sig_input(RECORD_DOMAIN, &s.body), &s.sig) {
            return Err(RecordError::BadSignature);
        }
        Ok(rec)
    }
}

impl SignedRecord {
    /// Decodes bytes our own store holds (verified on the way in): no signature, shape or
    /// canonical-encoding check, only that it decodes and is for this group.
    pub fn decode_trusted(bytes: &[u8], group: &str) -> Result<Record, RecordError> {
        let s: SignedRecord = bincode::deserialize(bytes).map_err(|_| RecordError::Undecodable)?;
        let rec: Record = bincode::deserialize(&s.body).map_err(|_| RecordError::Undecodable)?;
        if rec.group != group {
            return Err(RecordError::WrongGroup);
        }
        Ok(rec)
    }
}

/// The id of stored record bytes.
pub fn record_id(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..32].to_string()
}

// ---- invites ---------------------------------------------------------------------------------

/// "The holder may join `group`", signed by `issuer` (owner or admin at the time the join is
/// written, checked by every device from the doc).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub group: String,
    pub issuer: String,
    /// Unix seconds.
    pub expires: u64,
    pub nonce: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedInvite {
    pub body: Vec<u8>,
    pub sig: Vec<u8>,
}

impl Invite {
    pub fn sign(&self, account: &identity::Identity) -> SignedInvite {
        let body = bincode::serialize(self).expect("invite serialises");
        SignedInvite { sig: account.sign(&sig_input(INVITE_DOMAIN, &body)).to_vec(), body }
    }
}

impl SignedInvite {
    /// The invite, if its issuer's DID signed it. Expiry and the issuer's role are the caller's.
    pub fn open(&self) -> Option<Invite> {
        let inv: Invite = bincode::deserialize(&self.body).ok()?;
        (bincode::serialize(&inv).ok()? == self.body && verify_did(&inv.issuer, &sig_input(INVITE_DOMAIN, &self.body), &self.sig)).then_some(inv)
    }
}

// ---- host addresses (outside the doc) -----------------------------------------------------------

/// "Device `dev` of `did` hosts `group` at group endpoint `endpoint`", DID-signed. Only hosts'
/// addresses ever travel to members (U7); `at` orders announcements (newer replaces older).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAddr {
    pub group: String,
    pub did: String,
    pub dev: [u8; 32],
    pub endpoint: [u8; 32],
    pub relay: Option<String>,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedHostAddr {
    pub body: Vec<u8>,
    pub sig: Vec<u8>,
}

impl HostAddr {
    pub fn sign(&self, account: &identity::Identity) -> SignedHostAddr {
        let body = bincode::serialize(self).expect("host addr serialises");
        SignedHostAddr { sig: account.sign(&sig_input(HOST_ADDR_DOMAIN, &body)).to_vec(), body }
    }
}

impl SignedHostAddr {
    pub fn open(&self) -> Option<HostAddr> {
        let a: HostAddr = bincode::deserialize(&self.body).ok()?;
        if a.relay.as_ref().is_some_and(|r| r.len() > 300) {
            return None;
        }
        (bincode::serialize(&a).ok()? == self.body && verify_did(&a.did, &sig_input(HOST_ADDR_DOMAIN, &self.body), &self.sig)).then_some(a)
    }
}

// ---- endpoint proof (per connection) -------------------------------------------------------------

/// "Group endpoint `endpoint` is device `dev` of `did`", DID-signed. Sent first on every group
/// connection; the receiver checks `endpoint` is the connection's remote key (QUIC proved the
/// sender holds it). Never stored in the doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointAuth {
    pub did: String,
    pub dev: [u8; 32],
    pub endpoint: [u8; 32],
    pub sig: Vec<u8>,
}

fn auth_input(did: &str, dev: &[u8; 32], endpoint: &[u8; 32]) -> Vec<u8> {
    let mut v = AUTH_DOMAIN.to_vec();
    put_bytes(&mut v, did.as_bytes());
    v.extend_from_slice(dev);
    v.extend_from_slice(endpoint);
    v
}

impl EndpointAuth {
    pub fn new(account: &identity::Identity, dev: [u8; 32], endpoint: [u8; 32]) -> Self {
        let sig = account.sign(&auth_input(account.did(), &dev, &endpoint)).to_vec();
        Self { did: account.did().to_string(), dev, endpoint, sig }
    }

    /// Valid and made for the connection whose remote key is `remote`.
    pub fn verify(&self, remote: &[u8; 32]) -> bool {
        &self.endpoint == remote && verify_did(&self.did, &auth_input(&self.did, &self.dev, &self.endpoint), &self.sig)
    }
}

// ---- invite token -------------------------------------------------------------------------------------

/// The full invite: the signed grant (absent in a token for one's own other devices, which join as
/// the same DID) plus what to show and where the hosts are. G3: never shared as text any more (the
/// old `tlgroup1:` form is gone, U18): a host keeps it behind a short code (`code.rs`), a direct
/// invitation carries it over the private channel, own devices get it over self/3 (bincode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteToken {
    pub group: String,
    pub name: String,
    pub owner: String,
    pub invite: Option<SignedInvite>,
    pub hosts: Vec<SignedHostAddr>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct() -> identity::Identity {
        identity::generate().0
    }

    fn rec(a: &identity::Identity, group: &str, body: Body) -> Record {
        Record { v: 1, group: group.into(), author: a.did().into(), dev: [7; 32], at: 1000, seen: vec![(1, 2)], body }
    }

    // KIND: unit (G1 stage 1)
    #[test]
    fn records_verify_and_tampering_is_refused() {
        let a = acct();
        let r = rec(&a, "g", Body::Post { text: "hi".into(), reply_to: None });
        let bytes = r.sign(&a).to_bytes();
        assert_eq!(SignedRecord::open(&bytes, "g").unwrap(), r);
        assert_eq!(SignedRecord::open(&bytes, "other"), Err(RecordError::WrongGroup));
        // Any flipped byte: undecodable or a bad signature, never accepted.
        for i in 0..bytes.len() {
            let mut t = bytes.clone();
            t[i] ^= 0x41;
            assert!(SignedRecord::open(&t, "g").is_err(), "byte {i}");
        }
        // Trailing junk: a second id for the same record is refused.
        let mut t = bytes.clone();
        t.push(0);
        assert!(SignedRecord::open(&t, "g").is_err());
        // Claiming another author: the signature is the signer's, not the claimed DID's.
        let b = acct();
        let mut forged = rec(&a, "g", Body::Post { text: "as b".into(), reply_to: None });
        forged.author = b.did().into();
        let body = bincode::serialize(&forged).unwrap();
        let sig = a.sign(&sig_input(RECORD_DOMAIN, &body)).to_vec();
        let bytes = SignedRecord { body, sig }.to_bytes();
        assert_eq!(SignedRecord::open(&bytes, "g"), Err(RecordError::BadSignature));
    }

    // KIND: unit (G1 stage 1)
    #[test]
    fn malformed_records_are_refused() {
        let a = acct();
        let cases = [
            Body::Post { text: String::new(), reply_to: None },
            Body::Post { text: "x".repeat(MAX_TEXT + 1), reply_to: None },
            Body::Genesis { name: " ".into(), nonce: [0; 16], owner_name: "o".into() },
            Body::CallHostPermit { host: "d".repeat(201), host_dev: [0; 32], active: true },
        ];
        for b in cases {
            assert_eq!(SignedRecord::open(&rec(&a, "g", b).sign(&a).to_bytes(), "g"), Err(RecordError::Malformed));
        }
        let mut r = rec(&a, "g", Body::Leave);
        r.seen = vec![(5, 1), (3, 1)];
        assert_eq!(SignedRecord::open(&r.sign(&a).to_bytes(), "g"), Err(RecordError::Malformed));
    }

    // KIND: unit (G1 stage 1)
    #[test]
    fn invites_host_addrs_and_auth_verify() {
        let a = acct();
        let b = acct();
        let inv = Invite { group: "g".into(), issuer: a.did().into(), expires: 10, nonce: [1; 16] }.sign(&a);
        assert_eq!(inv.open().unwrap().issuer, a.did());
        let mut forged = Invite { group: "g".into(), issuer: b.did().into(), expires: 10, nonce: [1; 16] }.sign(&a);
        assert!(forged.open().is_none());
        forged.sig[0] ^= 1;
        assert!(forged.open().is_none());

        let ha = HostAddr { group: "g".into(), did: a.did().into(), dev: [1; 32], endpoint: [2; 32], relay: None, at: 5 }.sign(&a);
        assert!(ha.open().is_some());
        let mut t = ha.clone();
        t.body[3] ^= 1;
        assert!(t.open().is_none());

        let auth = EndpointAuth::new(&a, [1; 32], [9; 32]);
        assert!(auth.verify(&[9; 32]));
        assert!(!auth.verify(&[8; 32]), "made for another endpoint");
        let mut t = auth.clone();
        t.did = b.did().into();
        assert!(!t.verify(&[9; 32]));

        let tok = InviteToken { group: "g".into(), name: "n".into(), owner: a.did().into(), invite: Some(inv), hosts: vec![ha] };
        assert_eq!(bincode::deserialize::<InviteToken>(&bincode::serialize(&tok).unwrap()).unwrap(), tok);
    }

    #[test]
    fn group_ids_bind_owner_and_nonce() {
        assert_ne!(group_id("did:a", &[0; 16]), group_id("did:b", &[0; 16]));
        assert_ne!(group_id("did:a", &[0; 16]), group_id("did:a", &[1; 16]));
    }
}
