//! Group-call wire (docs/design/group-call-spec.md §1.1, §3.1, §6): the signed control
//! messages, their frames on the control stream, and the media datagram codecs.
//!
//! Pure like the rest of proto. Membership, permits and endpoints are the caller's checks
//! (`p2pcore::gcall::CallAuthority`); this module checks signatures, versions, shapes and
//! size bounds only.
//!
//! Every signed message is a `SignedBlob` over one internally tagged claim enum, with the one
//! domain `tinline/gcall/v1\0`. The `"t"` tag inside the signed payload keeps a signed `Join`
//! from being replayed as a `Leave` (their other fields overlap).
//!
//! Roster entries are compact (§9.1): an entry carries the joiner's DID, `issued_ms`, `nonce`
//! and the Join *signature* only. The signed Join payload is rebuilt from the Roster's own
//! `v`/`gid`/`call_id`/`host_gen`, so a member still checks that every listed DID signed a Join
//! for exactly this call and generation. A 12-entry Roster fits `MAX_CONTROL`
//! (`gcall_roster_of_12_fits_the_control_limit`).

use super::*;

/// ALPN of the per-call connection to the call host (§1.1).
pub const GCALL_ALPN: &[u8] = b"tinline/gcall/1";
pub(crate) const GCALL_DOMAIN: &[u8] = b"tinline/gcall/v1\0";
pub const GCALL_VERSION: u8 = 1;

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
    HostRotating,
    Removed,
    Superseded,
}

/// Signed by `host_ref.holder_did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallStart {
    pub v: u8,
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
    pub v: u8,
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
    #[serde(rename = "s")]
    pub slot: u8,
    #[serde(rename = "d")]
    pub did: String,
    #[serde(rename = "i")]
    pub issued_ms: u64,
    #[serde(rename = "n")]
    pub nonce: u64,
    /// The Join's signature, base64url.
    #[serde(rename = "g")]
    pub sig: String,
}

impl RosterEntry {
    /// `join` must be the verified claim inside `blob`.
    pub fn new(slot: u8, join: &Join, blob: &SignedBlob) -> Self {
        RosterEntry {
            slot,
            did: join.did.clone(),
            issued_ms: join.issued_ms,
            nonce: join.nonce,
            sig: blob.signature.clone(),
        }
    }
}

/// Signed by `host_did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Roster {
    pub v: u8,
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
    pub v: u8,
    pub gid: String,
    pub call_id: String,
    pub host_gen: u32,
    pub did: String,
    pub issued_ms: u64,
}

/// Signed by `host_did`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct End {
    pub v: u8,
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
    pub v: u8,
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
    pub v: u8,
    pub gid: String,
    pub call_id: String,
    pub starter_did: String,
    pub host: HostRef,
    pub reason: String,
}

/// A device's group endpoint note (§9 G2): "group endpoint `endpoint` is device `dev` of
/// `did`", signed by `did`. Sent with every `Join`, so a call host that holds no member
/// directory (it is not a chat host) can still check the connecting endpoint. Both keys are
/// base64url of 32 bytes. Only call hosts ever see it (U7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupNote {
    pub v: u8,
    pub did: String,
    pub dev: String,
    pub endpoint: String,
    pub issued_ms: u64,
}

impl GroupNote {
    pub fn dev_bytes(&self) -> Option<[u8; 32]> {
        dec32(&self.dev).ok()
    }
    pub fn endpoint_bytes(&self) -> Option<[u8; 32]> {
        dec32(&self.endpoint).ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Claim {
    Start(CallStart),
    Join(Join),
    Roster(Roster),
    Leave(Leave),
    End(End),
    HostRequest(HostRequest),
    HostRefusal(HostRefusal),
    Note(GroupNote),
}

impl Claim {
    fn signer(&self) -> &str {
        match self {
            Claim::Start(c) => &c.host_ref.holder_did,
            Claim::Join(c) => &c.did,
            Claim::Roster(c) => &c.host_did,
            Claim::Leave(c) => &c.did,
            Claim::End(c) => &c.host_did,
            Claim::HostRequest(c) => &c.starter_did,
            Claim::HostRefusal(c) => &c.host.holder_did,
            Claim::Note(c) => &c.did,
        }
    }
}

pub fn new_call_id() -> String {
    random16()
}

pub fn new_nonce() -> u64 {
    rand::rngs::OsRng.next_u64()
}

fn sign_claim(id: &Identity, claim: &Claim) -> SignedBlob {
    debug_assert_eq!(
        id.did(),
        claim.signer(),
        "gcall claims are signed by their claimed signer"
    );
    sign_blob(id, GCALL_DOMAIN, claim)
}

fn open_claim(blob: &SignedBlob) -> Result<Claim> {
    if blob.payload.len() > MAX_CONTROL * 2 {
        return Err(Error::FrameTooLarge);
    }
    open_blob(blob, GCALL_DOMAIN, |c: &Claim| c.signer())
}

pub(crate) fn text_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TEXT
}

fn check_common(v: u8, gid: &str, call_id: &str) -> Result<()> {
    if v != GCALL_VERSION {
        return Err(Error::UnknownVersion);
    }
    if !text_ok(gid) || dec(call_id).map(|b| b.len()) != Ok(CALL_ID_LEN) {
        return Err(Error::Decode);
    }
    Ok(())
}

fn host_ref_ok(r: &HostRef) -> bool {
    text_ok(&r.holder_did) && text_ok(&r.permit_digest)
}

pub fn sign_start(id: &Identity, start: CallStart) -> SignedBlob {
    sign_claim(id, &Claim::Start(start))
}

/// Signature (by `host_ref.holder_did`), version and bounds. Membership, the `host_call`
/// permit and generations are the caller's checks.
pub fn open_start(blob: &SignedBlob) -> Result<CallStart> {
    let Claim::Start(c) = open_claim(blob)? else {
        return Err(Error::UnexpectedMessage);
    };
    check_common(c.v, &c.gid, &c.call_id)?;
    let ok = c.host_gen >= 1
        && host_ref_ok(&c.host_ref)
        && text_ok(&c.starter_did)
        && c.k >= 1
        && (c.k as usize) < MAX_PARTICIPANTS
        && c.max_participants >= 2
        && c.max_participants as usize <= MAX_PARTICIPANTS
        && c.backups.len() <= MAX_BACKUPS
        && c.backups.iter().all(host_ref_ok)
        && c.meta_frontier.len() <= MAX_TEXT;
    if !ok {
        return Err(Error::Decode);
    }
    Ok(c)
}

/// A fresh Join with a random nonce.
pub fn sign_join(
    id: &Identity,
    gid: &str,
    call_id: &str,
    host_gen: u32,
    issued_ms: u64,
) -> (Join, SignedBlob) {
    let join = Join {
        v: GCALL_VERSION,
        gid: gid.into(),
        call_id: call_id.into(),
        host_gen,
        did: id.did().into(),
        issued_ms,
        nonce: new_nonce(),
    };
    let blob = sign_claim(id, &Claim::Join(join.clone()));
    (join, blob)
}

pub fn open_join(blob: &SignedBlob) -> Result<Join> {
    let Claim::Join(c) = open_claim(blob)? else {
        return Err(Error::UnexpectedMessage);
    };
    check_common(c.v, &c.gid, &c.call_id)?;
    Ok(c)
}

pub fn sign_roster(id: &Identity, roster: Roster) -> SignedBlob {
    sign_claim(id, &Claim::Roster(roster))
}

/// Host signature, bounds, unique slots and DIDs, and every entry's Join signature over this
/// Roster's call and generation.
pub fn open_roster(blob: &SignedBlob) -> Result<Roster> {
    let Claim::Roster(r) = open_claim(blob)? else {
        return Err(Error::UnexpectedMessage);
    };
    check_common(r.v, &r.gid, &r.call_id)?;
    if r.entries.len() > MAX_PARTICIPANTS {
        return Err(Error::Decode);
    }
    let mut slots = HashSet::new();
    let mut dids = HashSet::new();
    for e in &r.entries {
        if !slots.insert(e.slot) || !dids.insert(e.did.as_str()) {
            return Err(Error::Decode);
        }
        verify_entry(&r, e)?;
    }
    Ok(r)
}

/// The Join this entry stands for, as it was signed.
pub fn entry_join(r: &Roster, e: &RosterEntry) -> Join {
    Join {
        v: r.v,
        gid: r.gid.clone(),
        call_id: r.call_id.clone(),
        host_gen: r.host_gen,
        did: e.did.clone(),
        issued_ms: e.issued_ms,
        nonce: e.nonce,
    }
}

fn verify_entry(r: &Roster, e: &RosterEntry) -> Result<()> {
    let payload = serde_json::to_vec(&Claim::Join(entry_join(r, e))).expect("claims serialize");
    let sig: [u8; 64] = dec(&e.sig)?.try_into().map_err(|_| Error::Decode)?;
    let public = public_key_from_did(&e.did).ok_or(Error::Decode)?;
    if !verify(&public, &[GCALL_DOMAIN, &payload].concat(), &sig) {
        return Err(Error::BadSignature);
    }
    Ok(())
}

pub fn sign_leave(
    id: &Identity,
    gid: &str,
    call_id: &str,
    host_gen: u32,
    issued_ms: u64,
) -> SignedBlob {
    let leave = Leave {
        v: GCALL_VERSION,
        gid: gid.into(),
        call_id: call_id.into(),
        host_gen,
        did: id.did().into(),
        issued_ms,
    };
    sign_claim(id, &Claim::Leave(leave))
}

pub fn open_leave(blob: &SignedBlob) -> Result<Leave> {
    let Claim::Leave(c) = open_claim(blob)? else {
        return Err(Error::UnexpectedMessage);
    };
    check_common(c.v, &c.gid, &c.call_id)?;
    Ok(c)
}

pub fn sign_end(id: &Identity, end: End) -> SignedBlob {
    sign_claim(id, &Claim::End(end))
}

pub fn open_end(blob: &SignedBlob) -> Result<End> {
    let Claim::End(c) = open_claim(blob)? else {
        return Err(Error::UnexpectedMessage);
    };
    check_common(c.v, &c.gid, &c.call_id)?;
    if c.next_host.as_ref().is_some_and(|h| !host_ref_ok(h)) {
        return Err(Error::Decode);
    }
    Ok(c)
}

pub fn sign_host_request(id: &Identity, req: HostRequest) -> SignedBlob {
    sign_claim(id, &Claim::HostRequest(req))
}

/// Signature (by `starter_did`), version and bounds.
pub fn open_host_request(blob: &SignedBlob) -> Result<HostRequest> {
    let Claim::HostRequest(c) = open_claim(blob)? else {
        return Err(Error::UnexpectedMessage);
    };
    check_common(c.v, &c.gid, &c.call_id)?;
    let ok = text_ok(&c.starter_did)
        && host_ref_ok(&c.host)
        && c.backups.len() <= MAX_BACKUPS
        && c.backups.iter().all(host_ref_ok)
        && (2..=MAX_PARTICIPANTS).contains(&(c.expected as usize));
    if !ok {
        return Err(Error::Decode);
    }
    Ok(c)
}

pub fn sign_host_refusal(id: &Identity, refusal: HostRefusal) -> SignedBlob {
    sign_claim(id, &Claim::HostRefusal(refusal))
}

/// Signature (by `host.holder_did`), version and bounds.
pub fn open_host_refusal(blob: &SignedBlob) -> Result<HostRefusal> {
    let Claim::HostRefusal(c) = open_claim(blob)? else {
        return Err(Error::UnexpectedMessage);
    };
    check_common(c.v, &c.gid, &c.call_id)?;
    if !text_ok(&c.starter_did) || !host_ref_ok(&c.host) || c.reason.len() > MAX_TEXT {
        return Err(Error::Decode);
    }
    Ok(c)
}

pub fn sign_note(id: &Identity, dev: &[u8; 32], endpoint: &[u8; 32], issued_ms: u64) -> SignedBlob {
    let note = GroupNote {
        v: GCALL_VERSION,
        did: id.did().into(),
        dev: enc(dev),
        endpoint: enc(endpoint),
        issued_ms,
    };
    sign_claim(id, &Claim::Note(note))
}

/// Signature (by `did`), version and the two 32-byte keys.
pub fn open_note(blob: &SignedBlob) -> Result<GroupNote> {
    let Claim::Note(c) = open_claim(blob)? else {
        return Err(Error::UnexpectedMessage);
    };
    if c.v != GCALL_VERSION {
        return Err(Error::UnknownVersion);
    }
    if !text_ok(&c.did) || c.dev_bytes().is_none() || c.endpoint_bytes().is_none() {
        return Err(Error::Decode);
    }
    Ok(c)
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
        start: SignedBlob,
    },
    /// Member → host. `note` is the joiner's DID-signed group endpoint note (§9 G2), opaque
    /// here and checked by the host's `CallAuthority`.
    Join {
        join: SignedBlob,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<SignedBlob>,
    },
    Roster {
        roster: SignedBlob,
    },
    Leave {
        leave: SignedBlob,
    },
    End {
        end: SignedBlob,
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
