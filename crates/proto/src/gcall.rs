//! Group-call wire (docs/design/group-call-spec.md §1.1, §3.1, §6): the signed control
//! messages, their frames on the control stream, and the media datagram codecs.
//!
//! Pure like the rest of proto. Membership, permits and endpoints are the caller's checks
//! (`p2pcore::gcall::CallAuthority`); this module checks signatures, versions, shapes and
//! size bounds only.
//!
//! Every signed message is its own `Signable` with its own domain, so a signed `Join` cannot be
//! replayed as a `Leave` (their other fields overlap); `Signed::open` checks signature and shape.
//!
//! Roster entries are compact (§9.1): an entry carries the joiner's DID, `issued_ms`, `nonce`
//! and the Join *signature* only. The signed Join body is rebuilt from the Roster's own
//! `gid`/`call_id`/`host_gen` (postcard is deterministic), so a member still checks that every
//! listed DID signed a Join for exactly this call and generation. A 12-entry Roster fits `MAX_CONTROL`
//! (`gcall_roster_of_12_fits_the_control_limit`).

use super::*;

/// ALPN of the per-call connection to the call host (§1.1).
pub const GCALL_ALPN: &[u8] = b"tinline/gcall/1";
use cryptography::domains as d;

/// Largest control frame body (§6).
pub const MAX_CONTROL: usize = 4096;
/// Largest upstream datagram, type byte included (§6).
pub const MAX_UP_DATAGRAM: usize = 1200;
/// Launch cap on participants (Q1); also bounds Roster entries and `Active` slots.
pub const MAX_PARTICIPANTS: usize = 12;
/// `CallStart.backups` (§2.3).
pub const MAX_BACKUPS: usize = 2;
/// The 1:1 audio datagram header `[seq u32][level u8]` (crates/audio `HEADER`).
pub const AUDIO_HEADER: usize = 5;
/// Datagram type bytes (§1.1).
pub const UP_AUDIO: u8 = 0x01;
pub const DOWN_AUDIO: u8 = 0x02;
pub const UP_LEVEL: u8 = 0x03;

const CALL_ID_LEN: usize = 16;
pub(crate) const MAX_TEXT: usize = 128;

// ---- signed claims -----------------------------------------------------------------

/// A call-host candidate, without any transport key (U4 revised).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostRef {
    pub holder_did: String,
    pub device_slot: u64,
    pub permit_digest: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Ended,
    HostLeaving,
    Removed,
    Superseded,
}

/// Signed by `host_ref.holder_did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallStart {
    pub gid: String,
    /// 16 random bytes, base64url (`new_call_id`).
    pub call_id: String,
    pub host_gen: u32,
    pub host_ref: HostRef,
    pub starter_did: String,
    pub started_ms: u64,
    pub k: u8,
    pub max_participants: u8,
    pub backups: Vec<HostRef>,
    pub meta_frontier: String,
}

/// Signed by `did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Join {
    pub gid: String,
    pub call_id: String,
    pub host_gen: u32,
    pub did: String,
    pub issued_ms: u64,
    pub nonce: u64,
}

/// One participant in a `Roster`: the slot plus the joiner's Join, minus the fields the
/// Roster already carries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterEntry {
    pub slot: u8,
    pub did: String,
    pub issued_ms: u64,
    pub nonce: u64,
    /// The Join's signature (64 bytes).
    pub sig: Vec<u8>,
}

impl RosterEntry {
    /// `join` must be the verified claim inside `blob`.
    pub fn new(slot: u8, join: &Join, blob: &Signed<Join>) -> Self {
        RosterEntry {
            slot,
            did: join.did.clone(),
            issued_ms: join.issued_ms,
            nonce: join.nonce,
            sig: blob.sig().to_vec(),
        }
    }
}

/// Signed by `host_did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Roster {
    pub gid: String,
    pub call_id: String,
    pub host_gen: u32,
    pub host_did: String,
    /// Increments on every change.
    pub epoch: u32,
    pub entries: Vec<RosterEntry>,
}

/// Signed by `did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Leave {
    pub gid: String,
    pub call_id: String,
    pub host_gen: u32,
    pub did: String,
    pub issued_ms: u64,
}

/// Signed by `host_did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct End {
    pub gid: String,
    pub call_id: String,
    pub host_gen: u32,
    pub host_did: String,
    pub reason: EndReason,
    pub next_host: Option<HostRef>,
}

/// The starter asks one call-host candidate to host a new call (§2.3); relayed through the
/// chat hosts to that device. Signed by `starter_did`. `backups` go into the CallStart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostRequest {
    pub gid: String,
    pub call_id: String,
    pub starter_did: String,
    /// The device asked.
    pub host: HostRef,
    pub backups: Vec<HostRef>,
    /// Participants the starter expects (2..=`MAX_PARTICIPANTS`).
    pub expected: u8,
    pub issued_ms: u64,
}

/// The asked device declines a `HostRequest` (busy, paused, conditions, a phone whose user is
/// not in the call); relayed back to the starter. Signed by `host.holder_did`. A device that
/// accepts answers with the call's `CallStart` instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostRefusal {
    pub gid: String,
    pub call_id: String,
    pub starter_did: String,
    pub host: HostRef,
    pub reason: String,
}

/// A device's group endpoint note (§9 G2): "group endpoint `endpoint` is device `dev` of
/// `did`", signed by `did`. Sent with every `Join`, so a call host that holds no member
/// directory (it is not a chat host) can still check the connecting endpoint; group hosts take
/// the same statement from members (`p2pcore::group`). `issued_ms` is informational: no
/// rotation-generation check is made here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupNote {
    pub did: String,
    pub dev: [u8; 32],
    pub endpoint: [u8; 32],
    pub issued_ms: u64,
}

pub type SignedStart = Signed<CallStart>;
pub type SignedJoin = Signed<Join>;
pub type SignedRoster = Signed<Roster>;
pub type SignedLeave = Signed<Leave>;
pub type SignedEnd = Signed<End>;
pub type SignedHostRequest = Signed<HostRequest>;
pub type SignedHostRefusal = Signed<HostRefusal>;
pub type SignedNote = Signed<GroupNote>;

pub fn new_call_id() -> String {
    random16()
}

pub fn new_nonce() -> u64 {
    cryptography::rng::u64()
}

pub(crate) fn text_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TEXT
}

fn ok(cond: bool) -> Result<()> {
    if cond { Ok(()) } else { Err(Error::Decode) }
}

fn common_ok(gid: &str, call_id: &str) -> bool {
    text_ok(gid) && dec(call_id).map(|b| b.len()) == Ok(CALL_ID_LEN)
}

fn host_ref_ok(r: &HostRef) -> bool {
    text_ok(&r.holder_did) && text_ok(&r.permit_digest)
}

/// Signed by `host_ref.holder_did`. Membership, the `host_call` permit and generations are the
/// caller's checks.
impl Signable for CallStart {
    const DOMAIN: d::Domain = d::GCALL_START;
    const MAX_BODY: usize = MAX_CONTROL;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.host_ref.holder_did)
    }
    fn check(&self) -> Result<()> {
        let c = self;
        ok(common_ok(&c.gid, &c.call_id)
            && c.host_gen >= 1
            && host_ref_ok(&c.host_ref)
            && text_ok(&c.starter_did)
            && c.k >= 1
            && (c.k as usize) <= MAX_PARTICIPANTS
            && c.max_participants >= 2
            && c.max_participants as usize <= MAX_PARTICIPANTS
            && c.backups.len() <= MAX_BACKUPS
            && c.backups.iter().all(host_ref_ok)
            && c.meta_frontier.len() <= MAX_TEXT)
    }
}

impl Signable for Join {
    const DOMAIN: d::Domain = d::GCALL_JOIN;
    const MAX_BODY: usize = MAX_CONTROL;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.did)
    }
    fn check(&self) -> Result<()> {
        ok(common_ok(&self.gid, &self.call_id))
    }
}

/// Host signature, bounds, unique slots and DIDs, and every entry's Join signature over this
/// Roster's call and generation.
impl Signable for Roster {
    const DOMAIN: d::Domain = d::GCALL_ROSTER;
    const MAX_BODY: usize = MAX_CONTROL;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.host_did)
    }
    fn check(&self) -> Result<()> {
        ok(common_ok(&self.gid, &self.call_id) && self.entries.len() <= MAX_PARTICIPANTS)?;
        let mut slots = HashSet::new();
        let mut dids = HashSet::new();
        for e in &self.entries {
            ok(slots.insert(e.slot) && dids.insert(e.did.as_str()))?;
            verify_entry(self, e)?;
        }
        Ok(())
    }
}

impl Signable for Leave {
    const DOMAIN: d::Domain = d::GCALL_LEAVE;
    const MAX_BODY: usize = MAX_CONTROL;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.did)
    }
    fn check(&self) -> Result<()> {
        ok(common_ok(&self.gid, &self.call_id))
    }
}

impl Signable for End {
    const DOMAIN: d::Domain = d::GCALL_END;
    const MAX_BODY: usize = MAX_CONTROL;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.host_did)
    }
    fn check(&self) -> Result<()> {
        ok(common_ok(&self.gid, &self.call_id) && self.next_host.as_ref().is_none_or(host_ref_ok))
    }
}

/// Signed by `starter_did`.
impl Signable for HostRequest {
    const DOMAIN: d::Domain = d::GCALL_HOST_REQUEST;
    const MAX_BODY: usize = MAX_CONTROL;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.starter_did)
    }
    fn check(&self) -> Result<()> {
        let c = self;
        ok(common_ok(&c.gid, &c.call_id)
            && text_ok(&c.starter_did)
            && host_ref_ok(&c.host)
            && c.backups.len() <= MAX_BACKUPS
            && c.backups.iter().all(host_ref_ok)
            && (2..=MAX_PARTICIPANTS).contains(&(c.expected as usize)))
    }
}

/// Signed by `host.holder_did`.
impl Signable for HostRefusal {
    const DOMAIN: d::Domain = d::GCALL_HOST_REFUSAL;
    const MAX_BODY: usize = MAX_CONTROL;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.host.holder_did)
    }
    fn check(&self) -> Result<()> {
        let c = self;
        ok(common_ok(&c.gid, &c.call_id)
            && text_ok(&c.starter_did)
            && host_ref_ok(&c.host)
            && c.reason.len() <= MAX_TEXT)
    }
}

impl Signable for GroupNote {
    const DOMAIN: d::Domain = d::ENDPOINT_NOTE;
    const MAX_BODY: usize = 256;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.did)
    }
    fn check(&self) -> Result<()> {
        ok(text_ok(&self.did))
    }
}

/// A fresh Join with a random nonce.
pub fn sign_join(
    id: &Identity,
    gid: &str,
    call_id: &str,
    host_gen: u32,
    issued_ms: u64,
) -> Result<(Join, SignedJoin)> {
    let join = Join {
        gid: gid.into(),
        call_id: call_id.into(),
        host_gen,
        did: id.did().into(),
        issued_ms,
        nonce: new_nonce(),
    };
    let blob = Signed::sign(&join, id)?;
    Ok((join, blob))
}

/// The Join this entry stands for, as it was signed.
pub fn entry_join(r: &Roster, e: &RosterEntry) -> Join {
    Join {
        gid: r.gid.clone(),
        call_id: r.call_id.clone(),
        host_gen: r.host_gen,
        did: e.did.clone(),
        issued_ms: e.issued_ms,
        nonce: e.nonce,
    }
}

/// The entry's Join, reassembled as the envelope its joiner signed.
pub fn entry_signed_join(r: &Roster, e: &RosterEntry) -> Result<SignedJoin> {
    let body = cryptography::codec::to_bytes(&entry_join(r, e)).map_err(|_| Error::Decode)?;
    let sig: [u8; 64] = e.sig.as_slice().try_into().map_err(|_| Error::Decode)?;
    Ok(Signed::from_parts(body, sig))
}

fn verify_entry(r: &Roster, e: &RosterEntry) -> Result<()> {
    entry_signed_join(r, e)?.open().map(|_| ())
}

pub fn sign_leave(
    id: &Identity,
    gid: &str,
    call_id: &str,
    host_gen: u32,
    issued_ms: u64,
) -> Result<SignedLeave> {
    let leave = Leave {
        gid: gid.into(),
        call_id: call_id.into(),
        host_gen,
        did: id.did().into(),
        issued_ms,
    };
    Signed::sign(&leave, id)
}

pub fn sign_note(id: &Identity, dev: &[u8; 32], endpoint: &[u8; 32], issued_ms: u64) -> SignedNote {
    let note = GroupNote {
        did: id.did().into(),
        dev: *dev,
        endpoint: *endpoint,
        issued_ms,
    };
    Signed::sign(&note, id).expect("own note signs")
}

// ---- control frames ----------------------------------------------------------------

/// Frames on the call connection's bidirectional control stream: u32 BE length + JSON,
/// at most `MAX_CONTROL` bytes of body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum GroupCallMsg {
    /// Host → member, right after a Join is accepted: this generation's CallStart (the
    /// backups for redial; a takeover host's new CallStart reaches its members this way).
    Start {
        start: SignedStart,
    },
    /// Member → host. `note` is the joiner's DID-signed group endpoint note (§9 G2), opaque
    /// here and checked by the host's `CallAuthority`.
    Join {
        join: SignedJoin,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<SignedNote>,
    },
    Roster {
        roster: SignedRoster,
    },
    Leave {
        leave: SignedLeave,
    },
    End {
        end: SignedEnd,
    },
    /// Host → member, once per second; unsigned (authenticated stream).
    Heartbeat {
        epoch: u32,
    },
    /// Host → member whenever the forwarded set changes (G6-D1, G6-D2); unsigned (U14).
    Active {
        r#gen: u32,
        slots: Vec<u8>,
    },
}

/// `FrameTooLarge` if the body would exceed `MAX_CONTROL`.
pub fn encode_control(msg: &GroupCallMsg) -> Result<Vec<u8>> {
    let out = encode_json_frame(msg);
    if out.len() - 4 > MAX_CONTROL {
        return Err(Error::FrameTooLarge);
    }
    Ok(out)
}

/// `Ok(None)` = need more bytes. The length is checked before the body is buffered.
pub fn decode_control(buf: &[u8]) -> Result<Option<(GroupCallMsg, usize)>> {
    let r = decode_json_frame_max::<GroupCallMsg>(buf, MAX_CONTROL)?;
    if let Some((GroupCallMsg::Active { slots, .. }, _)) = &r
        && slots.len() > MAX_PARTICIPANTS
    {
        return Err(Error::Decode);
    }
    Ok(r)
}

// ---- datagrams ---------------------------------------------------------------------

/// A parsed member → host datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpDatagram<'a> {
    /// `[0x01][audio]`; `audio` is the 1:1 datagram `[seq][level][opus]`.
    Audio(&'a [u8]),
    /// `[0x03][level]`, sent every 500 ms while the sender's silence gate is closed. Never
    /// forwarded.
    Level(u8),
}

fn audio_ok(a: &[u8]) -> bool {
    a.len() > AUDIO_HEADER && a.len() < MAX_UP_DATAGRAM
}

pub fn parse_up(d: &[u8]) -> Result<UpDatagram<'_>> {
    if d.len() > MAX_UP_DATAGRAM {
        return Err(Error::FrameTooLarge);
    }
    match d {
        [UP_AUDIO, audio @ ..] if audio_ok(audio) => Ok(UpDatagram::Audio(audio)),
        [UP_LEVEL, level] => Ok(UpDatagram::Level(*level)),
        _ => Err(Error::Decode),
    }
}

pub fn up_audio(audio: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(audio.len() + 1);
    v.push(UP_AUDIO);
    v.extend_from_slice(audio);
    v
}

pub fn up_level(level: u8) -> [u8; 2] {
    [UP_LEVEL, level]
}

pub fn down_audio(slot: u8, audio: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(audio.len() + 2);
    v.extend_from_slice(&[DOWN_AUDIO, slot]);
    v.extend_from_slice(audio);
    v
}

/// `[0x02][slot][audio]` → `(slot, audio)`.
pub fn parse_down(d: &[u8]) -> Result<(u8, &[u8])> {
    if d.len() > MAX_UP_DATAGRAM + 1 {
        return Err(Error::FrameTooLarge);
    }
    match d {
        [DOWN_AUDIO, slot, audio @ ..] if audio_ok(audio) => Ok((*slot, audio)),
        _ => Err(Error::Decode),
    }
}

/// The sender-reported RFC 6464 level of an audio datagram already accepted by `parse_up`
/// or `parse_down`.
pub fn audio_level(audio: &[u8]) -> u8 {
    audio[4]
}

#[cfg(test)]
mod tests;
