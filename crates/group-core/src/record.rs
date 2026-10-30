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
    /// `text` may be empty only when the post carries an image (public group §5).
    Post { text: String, reply_to: Option<String>, image: Option<ImageRef> },
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
    /// (U21: a separate `host_call` capability, never implied by `HostPermit`). Per device
    /// the record in force by causality wins, like `HostPermit` (`state::causal_latest`).
    CallHostPermit { host: String, host_dev: [u8; 32], active: bool },
    /// The author's own device `dev` agrees to host group calls (or stops). Separate from
    /// `HostAccept`: agreeing to relay chat is not agreeing to carry calls (battery, uplink, the
    /// group endpoint shown to every caller), so a call host needs both a `CallHostPermit` and
    /// this.
    CallHostAccept { active: bool },
    /// The author joins an open group (`Open{active: true}` as of this record's frontier)
    /// without an invite; `name` as in `Join`. Inert in a group that was not open.
    OpenJoin { name: String },
    /// Owner or admin: anyone may join with `OpenJoin` (or no longer may). Causal: a later
    /// record that saw this one supersedes it; `(at, id)` only orders concurrent ones
    /// (`state.rs`). Members who joined stay.
    Open { active: bool },
    /// Owner or admin: only the owner and admins may post (a channel), or everyone again.
    /// Judged per post as of its frontier, like membership (U11).
    AdminsOnly { active: bool },
    /// A voice message post: the recording as one sealed blob, like a post's image, with the
    /// duration and waveform 1:1 voice messages carry. Shown, judged and stored exactly like a
    /// `Post` (day docs, U11 visibility, admins-only); see [`Body::is_post`]. A new variant at
    /// the end, so records written before it still decode.
    Voice { reply_to: Option<String>, voice: VoiceRef },
    /// A file post (`docs/design/group-files.md`): any file up to 2 GB as one sealed blob, named
    /// by its author-signed hash; `caption` may be empty. A post like `Post` and `Voice`.
    File { caption: String, reply_to: Option<String>, file: FileRef },
    /// "This device holds (or no longer holds) the file of post `post`" (group files §6). Not a
    /// shown post: it lives in the day doc of the file post (`day`), so it loads with the post;
    /// `hash` must be the post's `FileRef::hash` (otherwise it counts for nothing).
    FileHeld { post: String, day: String, hash: [u8; 32], held: bool },
}

impl Body {
    /// A conversation post (`Post`, `Voice` or `File`): lives in a day doc and is judged per post.
    pub fn is_post(&self) -> bool {
        matches!(self, Body::Post { .. } | Body::Voice { .. } | Body::File { .. })
    }

    /// Lives in a day doc: a post, or a file holder announcement (in its post's day).
    pub fn in_day_doc(&self) -> bool {
        self.is_post() || matches!(self, Body::FileHeld { .. })
    }

    /// The day doc a `FileHeld` belongs in (posts go by their own time).
    pub fn held_day(&self) -> Option<&str> {
        match self {
            Body::FileHeld { day, .. } => Some(day),
            _ => None,
        }
    }
}

/// Largest image blob a post may name, plaintext bytes (public group §5: senders stay under
/// 300 KB; a host refuses anything over this).
pub const MAX_IMAGE_BYTES: u64 = 1024 * 1024;
/// Longest side of a posted image, pixels.
pub const MAX_IMAGE_SIDE: u32 = 4096;
/// Most bytes of an image's colour preview (a 4x3 RGB grid, as 1:1 chat photos carry).
pub const MAX_IMAGE_PREVIEW: usize = 36;
/// The picture types a post may carry.
pub const IMAGE_MIMES: [&str; 3] = ["image/jpeg", "image/webp", "image/png"];

/// An image attached to a post (public group §5). The blob is the picture sealed with the
/// chunked blob cipher (`transfer::crypt`) under `key`, a fresh random key for this one blob,
/// as 1:1 chat attachments are: the key travels in the signed record, so every member (hosts
/// included) can open it, and the blob store holds only ciphertext. `hash` is the BLAKE3 of
/// the ciphertext (what iroh-blobs verifies while streaming); `size` the plaintext length.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    pub hash: [u8; 32],
    pub key: [u8; 32],
    pub size: u64,
    pub w: u32,
    pub h: u32,
    pub mime: String,
    /// A 4x3 RGB grid of its colours (empty if none), for the placeholder before it loads.
    pub preview: Vec<u8>,
}

impl ImageRef {
    /// Within the bounds every device checks (`Record::check_shape`).
    pub fn well_formed(&self) -> bool {
        self.size > 0
            && self.size <= MAX_IMAGE_BYTES
            && (1..=MAX_IMAGE_SIDE).contains(&self.w)
            && (1..=MAX_IMAGE_SIDE).contains(&self.h)
            && IMAGE_MIMES.contains(&self.mime.as_str())
            && self.preview.len() <= MAX_IMAGE_PREVIEW
    }
}

/// The type of a voice message blob: Ogg Opus, 16 kHz mono, as 1:1 voice messages.
pub const VOICE_MIME: &str = "audio/ogg";
/// Largest voice blob a post may name, plaintext bytes: the cap every device's voice decoder
/// applies to a file (1:1 voice messages have the same).
pub const MAX_VOICE_BYTES: u64 = 16 * 1024 * 1024;
/// Longest voice message a post may declare: what the voice decoder plays (30 minutes); the
/// apps stop recording at 15.
pub const MAX_VOICE_MS: u32 = 30 * 60 * 1000;
/// Bars of a voice message's waveform (the 1:1 format: exactly 64; fewer is tolerated).
pub const MAX_VOICE_BARS: usize = 64;

/// The recording of a voice post. Sealed and fetched as an image is ([`ImageRef`]): `hash` is the
/// ciphertext's BLAKE3, `key` the fresh per-blob key, `size` the plaintext length.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceRef {
    pub hash: [u8; 32],
    pub key: [u8; 32],
    pub size: u64,
    pub duration_ms: u32,
    /// Peaks 0..=255 for the bubble.
    pub waveform: Vec<u8>,
}

impl VoiceRef {
    /// Within the bounds every device checks (`Record::check_shape`).
    pub fn well_formed(&self) -> bool {
        self.size > 0
            && self.size <= MAX_VOICE_BYTES
            && (1..=MAX_VOICE_MS).contains(&self.duration_ms)
            && self.waveform.len() <= MAX_VOICE_BARS
    }

    /// The blob as the image runtime keeps it (local rows only, never on the wire): the same
    /// hash, key and size, `mime` = [`VOICE_MIME`], no picture shape.
    pub fn as_blob(&self) -> ImageRef {
        ImageRef { hash: self.hash, key: self.key, size: self.size, w: 0, h: 0, mime: VOICE_MIME.to_string(), preview: Vec::new() }
    }
}

/// Largest file a post may name, plaintext bytes: 1:1's `transfer::crypt::MAX_FILE` (2 GiB).
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Longest file name, bytes.
pub const MAX_FILE_NAME: usize = 255;
/// Longest file type.
pub const MAX_FILE_MIME: usize = 100;
/// Group files §4: hosts fetch and keep files up to this many bytes by themselves, like
/// pictures, within the group budget. The current choice is option B: hosts keep every file
/// (`MAX_FILE_BYTES`). Options A (a smaller cap) and C (0) also need the on-demand host relay,
/// which is not built now (`docs/design/group-files.md` §4). One value on every device, so
/// members can show `kept_by_hosts`.
pub const HOST_KEEP_FILE_MAX: u64 = MAX_FILE_BYTES;
/// The local row mime of a file blob in the image runtime (never on the wire).
pub const FILE_BLOB_MIME: &str = "application/x-tinline-file";

/// The file of a file post. Sealed as 1:1 attachments are (`transfer::crypt` under `key`, a
/// fresh key for this blob); `hash` is the BLAKE3 (bao) root of the sealed blob, computed by the
/// sender while sealing and signed with the post: every download is verified against it chunk
/// by chunk. `size` is the plaintext length.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    pub hash: [u8; 32],
    pub key: [u8; 32],
    pub size: u64,
    pub name: String,
    pub mime: String,
}

/// A file name every device accepts: 1..=255 bytes, a single path component (no `/`, `\`,
/// NUL or other control characters, not `.` or `..`).
pub fn file_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FILE_NAME
        && name != "."
        && name != ".."
        && !name.chars().any(|c| c == '/' || c == '\\' || c.is_control())
}

impl FileRef {
    /// Within the bounds every device checks (`Record::check_shape`).
    pub fn well_formed(&self) -> bool {
        self.size <= MAX_FILE_BYTES
            && file_name_ok(&self.name)
            && self.mime.len() <= MAX_FILE_MIME
            && self.mime.is_ascii()
            && !self.mime.chars().any(|c| c.is_ascii_control())
    }

    /// Hosts keep this file (group files §4; every file under option B).
    pub fn kept_by_hosts(&self) -> bool {
        self.size <= HOST_KEEP_FILE_MAX
    }

    /// The blob as the image runtime keeps it (local rows only): hash, key, size, and
    /// [`FILE_BLOB_MIME`]; the row carries the `FileRef` itself.
    pub fn as_blob(&self) -> ImageRef {
        ImageRef { hash: self.hash, key: self.key, size: self.size, w: 0, h: 0, mime: FILE_BLOB_MIME.to_string(), preview: Vec::new() }
    }
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
            Body::Post { text, reply_to, image }
                if text.len() > MAX_TEXT
                    || (text.is_empty() && image.is_none())
                    || reply_to.as_ref().is_some_and(|r| r.len() > 64)
                    || image.as_ref().is_some_and(|i| !i.well_formed()) =>
            {
                bad
            }
            Body::Voice { reply_to, voice } if reply_to.as_ref().is_some_and(|r| r.len() > 64) || !voice.well_formed() => bad,
            Body::File { caption, reply_to, file }
                if caption.len() > MAX_TEXT || reply_to.as_ref().is_some_and(|r| r.len() > 64) || !file.well_formed() =>
            {
                bad
            }
            Body::FileHeld { post, day, .. } if post.is_empty() || post.len() > 64 || !crate::doc::day_valid(day) => bad,
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
/// Longest member DID a `Presence` lists. A member's DID is a `did:key` that signs its records
/// (56 bytes for ed25519), so this never drops a real member; it bounds a full frame to
/// 500 × (64 + 11) bytes plus the header, well inside `MAX_BODY`.
pub const MAX_PRESENCE_DID: usize = 64;

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
    /// 500 DIDs of at most `MAX_PRESENCE_DID` bytes plus their times, with room.
    const MAX_BODY: usize = 64 * 1024;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.host)
    }
    fn check(&self) -> proto::Result<()> {
        if self.online.len() > MAX_PRESENCE || self.online.iter().any(|(d, _)| d.len() > MAX_PRESENCE_DID) || self.host.len() > 200 || self.group.len() > 200 {
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
