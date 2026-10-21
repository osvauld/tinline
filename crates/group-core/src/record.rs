//! Group records: every group op (genesis, join, post, admin grant, flag, host permit, ...) is
//! one DID-signed record. Nothing in a record is a network key (U4 revised): the author is a DID,
//! the device is the per-install random device ID (`dev`, U16), and the record's Loro peer id
//! must be `writer_id(author, dev)`.
//!
//! A record is stored as `SignedRecord` bytes (`proto::Signed`); its id is the hex BLAKE3 of
//! those bytes, so the id binds the content and the signature. Records are immutable: an edit
//! would be a new record.
//!
//! Outside the doc (never in a record): invites (`SignedInvite`, only its signed grant goes into
//! a `Join`), host addresses (`SignedHostAddr`) and the per-connection endpoint proof
//! (`EndpointAuth`, the gcall endpoint note). Those carry group endpoints and travel only
//! member <-> host (U7).

use cryptography::{domains, hash};
use proto::{Signable, Signed, Signer};
use serde::{Deserialize, Serialize};

/// Largest stored record (a post's text is capped well below).
pub const MAX_RECORD: usize = 64 * 1024;
/// Most bytes of a signed invite or host address body (both are a few hundred bytes).
const MAX_SMALL_BODY: usize = 1024;
/// Most bytes of an encoded `InviteToken` (an invite plus a handful of host addresses).
pub const MAX_TOKEN: usize = 64 * 1024;
pub const MAX_TEXT: usize = 20_000;
pub const MAX_NAME: usize = 100;
/// A `seen` frontier with more writers than this is refused (bounds a record's size).
pub const MAX_SEEN: usize = 1024;

/// A group's id: hex `hash::hasher(GROUP_ID)` over the owner's DID and the genesis nonce.
pub fn group_id(owner: &str, nonce: &[u8; 16]) -> String {
    hash::hasher(domains::GROUP_ID).part(owner.as_bytes()).part(nonce).hex()
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
    /// The author joins an open group (`Open{active: true}` as of this record's frontier)
    /// without an invite; `name` as in `Join`. Inert in a group that was not open.
    OpenJoin { name: String },
    /// Owner or admin: anyone may join with `OpenJoin` (or no longer may). Latest by
    /// `(at, id)` among those inside a frontier counts. Members who joined stay.
    Open { active: bool },
    /// Owner or admin: only the owner and admins may post (a channel), or everyone again.
    /// Judged per post as of its frontier, like membership (U11).
    AdminsOnly { active: bool },
}

/// One group op, as signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
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

pub type SignedRecord = Signed<Record>;

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

impl Signable for Record {
    const DOMAIN: domains::Domain = domains::GROUP_RECORD;
    const MAX_BODY: usize = MAX_RECORD - proto::signed::SIG_LEN;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.author)
    }
    fn check(&self) -> proto::Result<()> {
        self.check_shape().map_err(|_| proto::Error::Decode)
    }
}

impl Record {
    fn check_shape(&self) -> Result<(), RecordError> {
        let bad = Err(RecordError::Malformed);
        if self.author.is_empty() || self.author.len() > 200 || self.seen.len() > MAX_SEEN {
            return bad;
        }
        if self.seen.windows(2).any(|w| w[0].0 >= w[1].0) {
            return bad;
        }
        match &self.body {
            Body::Genesis { name, owner_name, .. } if name.len() > MAX_NAME || name.trim().is_empty() || owner_name.len() > MAX_NAME => bad,
            Body::Join { name, .. } | Body::OpenJoin { name } if name.len() > MAX_NAME => bad,
            Body::Post { text, reply_to } if text.len() > MAX_TEXT || text.is_empty() || reply_to.as_ref().is_some_and(|r| r.len() > 64) => bad,
            Body::Admin { member, .. } | Body::Flag { member } if member.len() > 200 => bad,
            Body::HostPermit { host, .. } | Body::CallHostPermit { host, .. } if host.len() > 200 => bad,
            _ => Ok(()),
        }
    }

    /// Signs as `account` (whose DID must be `self.author`).
    pub fn sign(&self, account: &identity::Identity) -> SignedRecord {
        Signed::sign(self, account).expect("own record signs")
    }
}

/// Decodes stored or received bytes and verifies them: shape, group, and the author's DID
/// signature.
pub fn open_record(bytes: &[u8], group: &str) -> Result<Record, RecordError> {
    let s = SignedRecord::from_bytes(bytes).map_err(|_| RecordError::Malformed)?;
    let rec = s.open().map_err(|e| match e {
        proto::Error::BadSignature => RecordError::BadSignature,
        proto::Error::TooLarge => RecordError::Malformed,
        _ => match s.peek() {
            Ok(r) if r.check_shape().is_err() => RecordError::Malformed,
            _ => RecordError::Undecodable,
        },
    })?;
    if rec.group != group {
        return Err(RecordError::WrongGroup);
    }
    Ok(rec)
}

/// Decodes bytes our own store holds (verified on the way in): no signature, shape or
/// canonical-encoding check, only that it decodes and is for this group.
pub fn decode_trusted(bytes: &[u8], group: &str) -> Result<Record, RecordError> {
    let rec = SignedRecord::from_bytes(bytes)
        .and_then(|s| s.peek())
        .map_err(|_| RecordError::Undecodable)?;
    if rec.group != group {
        return Err(RecordError::WrongGroup);
    }
    Ok(rec)
}

/// The id of stored record bytes.
pub fn record_id(bytes: &[u8]) -> String {
    hex::encode(&hash::content(domains::GROUP_RECORD_ID, bytes)[..16])
}

// ---- invites ---------------------------------------------------------------------------------

/// "Whoever holds this may join `group`", issued by an owner or admin (checked by every device
/// from the doc, as of the join). A bearer statement (`Subject::Anyone`) until it expires (D17):
/// revoking a short code does not take back an invite already fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupJoin {
    pub group: String,
}

impl proto::IssuedClaim for GroupJoin {
    const DOMAIN: domains::Domain = domains::GROUP_INVITE;
    const MAX_TTL_MS: u64 = MAX_INVITE_TTL_MS;
    const MAX_BODY: usize = MAX_SMALL_BODY;
}

/// Longest an invite may live (`INVITE_TTL` is a week).
pub const MAX_INVITE_TTL_MS: u64 = 30 * 24 * 3600 * 1000;

pub type Invite = proto::Issued<GroupJoin>;
pub type SignedInvite = Signed<Invite>;

/// A fresh invite to `group` from `account`, valid `ttl_ms` from `now_ms`.
pub fn issue_invite(account: &identity::Identity, group: &str, now_ms: u64, ttl_ms: u64) -> SignedInvite {
    proto::issued::issue(account, proto::Subject::Anyone, now_ms, ttl_ms, GroupJoin { group: group.to_string() })
        .expect("own invite signs")
}

/// An invite `holder` redeems at `now_ms` (bearer: anyone may; `store` says what is spent), for
/// `group`. Whether its issuer is an admin is the caller's check, from the doc.
pub fn redeem_invite(invite: &SignedInvite, group: &str, holder: &str, now_ms: u64, store: &dyn proto::IssuanceStore) -> Option<Invite> {
    let inv = invite.redeem(&proto::Expect { iss: None, holder, now_ms }, store).ok()?;
    (inv.claim.group == group && inv.sub == proto::Subject::Anyone).then_some(inv)
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

impl Signable for HostAddr {
    const DOMAIN: domains::Domain = domains::GROUP_HOST_ADDR;
    const MAX_BODY: usize = MAX_SMALL_BODY;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.did)
    }
    fn check(&self) -> proto::Result<()> {
        if self.relay.as_ref().is_some_and(|r| r.len() > 300) {
            return Err(proto::Error::Decode);
        }
        Ok(())
    }
}

pub type SignedHostAddr = Signed<HostAddr>;

// ---- presence (outside the doc, never stored) -------------------------------------------------

/// Most members one `Presence` lists (public group §3: larger groups show "500+ online").
pub const MAX_PRESENCE: usize = 500;

/// "Host device `dev` of `host` saw these members online", DID-signed by the host (public group
/// §3). `online` is `(member DID, last seen)` and `at` the host's clock when it signed, so a
/// receiver judges age as `at - last seen` without trusting clocks to agree. `seq` grows per
/// host device (the newest frame per host wins; a relay drops one it has). In memory only:
/// never a record, never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presence {
    pub group: String,
    pub host: String,
    pub dev: [u8; 32],
    pub online: Vec<(String, u64)>,
    pub seq: u64,
    pub at: u64,
}

impl Signable for Presence {
    const DOMAIN: domains::Domain = domains::GROUP_PRESENCE;
    /// 500 DIDs of ~60 bytes plus their times, with room.
    const MAX_BODY: usize = 64 * 1024;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.host)
    }
    fn check(&self) -> proto::Result<()> {
        if self.online.len() > MAX_PRESENCE || self.online.iter().any(|(d, _)| d.len() > 200) {
            return Err(proto::Error::Decode);
        }
        Ok(())
    }
}

pub type SignedPresence = Signed<Presence>;

// ---- endpoint proof (per connection) -------------------------------------------------------------

/// "Group endpoint `endpoint` is device `dev` of `did`", DID-signed: the same statement as the
/// gcall endpoint note. Sent first on every group connection; the receiver checks `endpoint` is
/// the connection's remote key (QUIC proved the sender holds it). Never stored in the doc.
pub type EndpointAuth = proto::gcall::SignedNote;

/// Our endpoint proof for a connection from `endpoint`.
pub fn endpoint_auth(
    account: &identity::Identity,
    dev: [u8; 32],
    endpoint: [u8; 32],
) -> EndpointAuth {
    proto::gcall::sign_note(account, &dev, &endpoint, crdt::now_ms())
}

/// The proof's statement, if valid and made for the connection whose remote key is `remote`.
pub fn verify_endpoint(auth: &EndpointAuth, remote: &[u8; 32]) -> Option<proto::gcall::GroupNote> {
    auth.open().ok().filter(|n| &n.endpoint == remote)
}

// ---- invite token -------------------------------------------------------------------------------------

/// The full invite: the signed grant (absent in a token for one's own other devices, which join as
/// the same DID) plus what to show and where the hosts are. G3: never shared as text any more (the
/// old `tlgroup1:` form is gone, U18): a host keeps it behind a short code (`code.rs`), a direct
/// invitation carries it over the private channel, own devices get it over self/5 (postcard).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteToken {
    pub group: String,
    pub name: String,
    pub owner: String,
    pub invite: Option<SignedInvite>,
    pub hosts: Vec<SignedHostAddr>,
    /// Fetched with an open link (`invite` is `None`): anyone may join with `OpenJoin` while
    /// the group is open. `false` with no invite: a device token (own DID only).
    pub open: bool,
}

#[cfg(test)]
mod tests;
