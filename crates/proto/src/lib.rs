//! P2P call protocol: who is who, who may call whom. Pure — no I/O, no clock, no storage.
//!
//! Two keys per phone. The osvauld `Identity` (DID) is the person; a random per-install
//! device key is the iroh endpoint key, and the QUIC handshake already proves the peer holds
//! it. An `Attestation` (DID-signed) binds the two, and every check below ties the
//! attested device to the `remote_device` the transport authenticated — that is what stops a
//! relay or a replayer from substituting itself.
//!
//! Like courier, `now`, nonce/grant-id sets and the contact list are inputs: the caller
//! loads them before calling and persists the results (`NewContact::redeemed_nonce`) after.

use std::collections::HashSet;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::SigningKey;
use identity::{Identity, public_key_from_did, verify};
use rand::RngCore;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub const ALPN: &[u8] = b"osvauld/p2p/0";

const ATTEST_DOMAIN: &[u8] = b"osvauld/p2p/attest/v1\0";
const INVITE_DOMAIN: &[u8] = b"osvauld/p2p/invite/v1\0";
const GRANT_DOMAIN: &[u8] = b"osvauld/p2p/grant/v1\0";
const INVITE_V2_DOMAIN: &[u8] = b"osvauld/p2p/invite/v2\0";
const BIND_DOMAIN: &[u8] = b"osvauld/p2p/bind/v1\0";

const VERSION: u8 = 1;
const TICKET_PREFIX: &str = "osvc1.";
const TICKET_PREFIX_V2: &str = "OSVC2:";
const CAP_CALL: &str = "call";
pub const MAX_FRAME: usize = 64 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("bad signature")]
    BadSignature,
    #[error("expired")]
    Expired,
    #[error("issuer is not who was expected")]
    WrongIssuer,
    #[error("holder is not who was expected")]
    WrongHolder,
    #[error("attested device does not match the connected device")]
    DeviceMismatch,
    #[error("invite already redeemed")]
    Replayed,
    #[error("grant revoked")]
    Revoked,
    #[error("caller is not a contact")]
    NotContact,
    #[error("unknown version or prefix")]
    UnknownVersion,
    #[error("malformed data")]
    Decode,
    #[error("frame exceeds {MAX_FRAME} bytes")]
    FrameTooLarge,
    #[error("ticket fields do not match the signed invite")]
    TicketMismatch,
    #[error("unsupported capability")]
    BadCapability,
    #[error("unexpected message for this step")]
    UnexpectedMessage,
    #[error("cannot add yourself")]
    SelfContact,
}

pub type Result<T> = std::result::Result<T, Error>;

fn enc(bytes: impl AsRef<[u8]>) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn dec(text: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text).map_err(|_| Error::Decode)
}

fn dec32(text: &str) -> Result<[u8; 32]> {
    dec(text)?.try_into().map_err(|_| Error::Decode)
}

fn random16() -> String {
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    enc(b)
}

// ---- device keys -------------------------------------------------------------------

/// Device key text form is `enc(32 bytes)`; the secret's storage is the caller's job.
pub fn new_device_secret() -> [u8; 32] {
    let mut s = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut s);
    s
}

pub fn device_public(secret: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(secret).verifying_key().to_bytes()
}

pub fn device_to_text(device: &[u8; 32]) -> String {
    enc(device)
}

pub fn device_from_text(text: &str) -> Result<[u8; 32]> {
    dec32(text)
}

// ---- peer-supplied text ------------------------------------------------------------

/// Longest display name we keep, in chars.
pub const MAX_NAME_CHARS: usize = 64;

/// A name from a peer, made safe to show: control characters and bidi overrides/isolates (which
/// can reorder the text around them) are dropped, and the rest is cut to `MAX_NAME_CHARS`.
pub fn sanitize_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}')
        })
        .take(MAX_NAME_CHARS)
        .collect();
    clean.trim().to_string()
}

// ---- signed envelope ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedBlob {
    pub payload: String,
    pub signature: String,
}

pub type SignedAttestation = SignedBlob;
pub type SignedGrant = SignedBlob;

fn sign_blob<T: Serialize>(id: &Identity, domain: &[u8], claim: &T) -> SignedBlob {
    let payload = serde_json::to_vec(claim).expect("claims serialize");
    SignedBlob {
        signature: enc(id.sign(&[domain, &payload].concat())),
        payload: enc(payload),
    }
}

/// The signer is whoever the payload *claims* (`issuer_of`); the key comes from that DID, so
/// a valid signature proves the claimed DID wrote it. Callers then check it is the DID they
/// expected.
fn open_blob<T: DeserializeOwned>(
    blob: &SignedBlob,
    domain: &[u8],
    issuer_of: impl Fn(&T) -> &str,
) -> Result<T> {
    let payload = dec(&blob.payload)?;
    let sig: [u8; 64] = dec(&blob.signature)?
        .try_into()
        .map_err(|_| Error::Decode)?;
    let claim: T = serde_json::from_slice(&payload).map_err(|_| Error::Decode)?;
    let public = public_key_from_did(issuer_of(&claim)).ok_or(Error::Decode)?;
    if !verify(&public, &[domain, &payload].concat(), &sig) {
        return Err(Error::BadSignature);
    }
    Ok(claim)
}

fn check_version(v: u8) -> Result<()> {
    if v == VERSION {
        Ok(())
    } else {
        Err(Error::UnknownVersion)
    }
}

// ---- attestation -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Attestation {
    pub v: u8,
    pub did: String,
    pub device: String,
    pub iat: u64,
}

impl Attestation {
    pub fn device_key(&self) -> Result<[u8; 32]> {
        dec32(&self.device)
    }
}

/// No expiry: it asserts a fact about key custody. There is no device revocation; removing a
/// contact is a local block (see docs/protocol.md). `iat` is informational (callers may age it
/// out).
pub fn attest(id: &Identity, device: [u8; 32], now: u64) -> SignedAttestation {
    let claim = Attestation {
        v: VERSION,
        did: id.did().to_string(),
        device: enc(device),
        iat: now,
    };
    sign_blob(id, ATTEST_DOMAIN, &claim)
}

pub fn verify_attestation(a: &SignedAttestation) -> Result<Attestation> {
    let claim: Attestation = open_blob(a, ATTEST_DOMAIN, |c: &Attestation| &c.did)?;
    check_version(claim.v)?;
    claim.device_key()?;
    Ok(claim)
}

// ---- contact ticket ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InviteClaim {
    pub v: u8,
    pub iss: String,
    pub nonce: String,
    pub iat: u64,
    pub exp: u64,
    pub device: String,
    pub name: String,
    pub relay: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactTicket {
    pub v: u8,
    pub did: String,
    pub name: String,
    pub device: String,
    pub relay: Option<String>,
    pub invite: SignedBlob,
}

fn open_invite(blob: &SignedBlob) -> Result<InviteClaim> {
    if is_v2_payload(blob)? {
        return open_invite_v2(blob);
    }
    let claim: InviteClaim = open_blob(blob, INVITE_DOMAIN, |c: &InviteClaim| &c.iss)?;
    check_version(claim.v)?;
    if dec(&claim.nonce)?.len() != 16 {
        return Err(Error::Decode);
    }
    Ok(claim)
}

// ---- compact v2 invite -------------------------------------------------------------
//
// Binary body (what the QR carries, after the `OSVC2:` prefix, base32 uppercase, no pad):
//   signing_pk[32] | device[32] | nonce[6] | exp[3] | flags[1] | name[len] | relay_ext | sig[64]
// - exp: minutes since 2026-01-01T00:00:00Z, u24 big endian (rounded up when issuing).
// - flags: high 3 bits relay code (0 none, 1..=4 known n0 relays, 7 custom with a trailing
//   u8 length + URL), low 5 bits name length in bytes (so names are capped at 31 bytes).
// - sig: Ed25519 by the DID key over `INVITE_V2_DOMAIN || 0x02 || everything before sig`.
// The issuer DID is `did_from_public_key(signing_pk)`; `iat` is not carried (decodes as 0).
// Inside `SignedBlob` the payload is `b64url(0x02 || body-before-sig)`; a v1 payload is JSON
// and starts with `{`, so the first byte tells the versions apart.

const V2_BYTE: u8 = 2;
const V2_EPOCH: u64 = 1_767_225_600;
const V2_NONCE_LEN: usize = 6;
const V2_MAX_NAME: usize = 31;
const KNOWN_RELAYS: [&str; 4] = [
    "https://use1-1.relay.n0.iroh.link./",
    "https://usw1-1.relay.n0.iroh.link./",
    "https://euc1-1.relay.n0.iroh.link./",
    "https://aps1-1.relay.n0.iroh.link./",
];
const RELAY_CUSTOM: u8 = 7;
const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn b32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[(acc >> bits & 31) as usize] as char);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(B32[(acc << (5 - bits) & 31) as usize] as char);
    }
    out
}

fn b32_decode(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = B32
            .iter()
            .position(|&x| x == c.to_ascii_uppercase())
            .ok_or(Error::Decode)? as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // Leftover bits are padding and must be zero (canonical encoding).
    if bits >= 5 || acc != 0 {
        return Err(Error::Decode);
    }
    Ok(out)
}

fn is_v2_payload(blob: &SignedBlob) -> Result<bool> {
    Ok(dec(&blob.payload)?.first().copied() == Some(V2_BYTE))
}

struct V2Body {
    pk: [u8; 32],
    device: [u8; 32],
    nonce: [u8; V2_NONCE_LEN],
    exp: u64,
    name: String,
    relay: Option<String>,
    /// Bytes consumed from the body, i.e. where the signature starts.
    len: usize,
}

fn parse_v2_body(b: &[u8]) -> Result<V2Body> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8]> {
        let s = b.get(at..at + n).ok_or(Error::Decode)?;
        at += n;
        Ok(s)
    };
    let pk: [u8; 32] = take(32)?.try_into().unwrap();
    let device: [u8; 32] = take(32)?.try_into().unwrap();
    let nonce: [u8; V2_NONCE_LEN] = take(V2_NONCE_LEN)?.try_into().unwrap();
    let e = take(3)?;
    let exp = V2_EPOCH + 60 * ((e[0] as u64) << 16 | (e[1] as u64) << 8 | e[2] as u64);
    let flags = take(1)?[0];
    let name = sanitize_name(
        std::str::from_utf8(take((flags & 31) as usize)?).map_err(|_| Error::Decode)?,
    );
    let relay = match flags >> 5 {
        0 => None,
        c @ 1..=4 => Some(KNOWN_RELAYS[c as usize - 1].to_string()),
        RELAY_CUSTOM => {
            let n = take(1)?[0] as usize;
            Some(
                std::str::from_utf8(take(n)?)
                    .map_err(|_| Error::Decode)?
                    .to_string(),
            )
        }
        _ => return Err(Error::Decode),
    };
    Ok(V2Body {
        pk,
        device,
        nonce,
        exp,
        name,
        relay,
        len: at,
    })
}

fn open_invite_v2(blob: &SignedBlob) -> Result<InviteClaim> {
    let payload = dec(&blob.payload)?;
    let sig: [u8; 64] = dec(&blob.signature)?
        .try_into()
        .map_err(|_| Error::Decode)?;
    let body = parse_v2_body(&payload[1..])?;
    if body.len + 1 != payload.len() {
        return Err(Error::Decode);
    }
    if !verify(&body.pk, &[INVITE_V2_DOMAIN, &payload].concat(), &sig) {
        return Err(Error::BadSignature);
    }
    Ok(InviteClaim {
        v: 2,
        iss: identity::did_from_public_key(&body.pk),
        nonce: enc(body.nonce),
        iat: 0,
        exp: body.exp,
        device: enc(body.device),
        name: body.name,
        relay: body.relay,
    })
}

impl ContactTicket {
    /// Text form. A ticket we issue is v2 (`OSVC2:` + base32, QR-friendly); a ticket parsed
    /// from `osvc1.` text can't be re-signed, so it keeps its v1 form.
    pub fn to_text(&self) -> String {
        if let Ok(true) = is_v2_payload(&self.invite)
            && let (Ok(payload), Ok(sig)) = (dec(&self.invite.payload), dec(&self.invite.signature))
        {
            let bytes = [&payload[1..], &sig[..]].concat();
            return format!("{TICKET_PREFIX_V2}{}", b32_encode(&bytes));
        }
        let json = serde_json::to_vec(self).expect("ticket serializes");
        format!("{TICKET_PREFIX}{}", enc(json))
    }

    /// Accepts v1 (`osvc1.`) and v2 (`OSVC2:`, case-insensitive), tolerating whitespace.
    pub fn from_text(text: &str) -> Result<Self> {
        let text = text.trim();
        if let Some(head) = text.get(..TICKET_PREFIX_V2.len())
            && head.eq_ignore_ascii_case(TICKET_PREFIX_V2)
        {
            return Self::from_v2(&text[TICKET_PREFIX_V2.len()..]);
        }
        let body = text
            .strip_prefix(TICKET_PREFIX)
            .ok_or(Error::UnknownVersion)?;
        let t: Self = serde_json::from_slice(&dec(body)?).map_err(|_| Error::Decode)?;
        check_version(t.v)?;
        Ok(t)
    }

    fn from_v2(text: &str) -> Result<Self> {
        let bytes = b32_decode(text)?;
        let split = bytes.len().checked_sub(64).ok_or(Error::Decode)?;
        let payload = [&[V2_BYTE][..], &bytes[..split]].concat();
        let body = parse_v2_body(&payload[1..])?;
        if body.len + 1 != payload.len() {
            return Err(Error::Decode);
        }
        Ok(Self {
            v: 2,
            did: identity::did_from_public_key(&body.pk),
            name: body.name,
            device: enc(body.device),
            relay: body.relay,
            invite: SignedBlob {
                payload: enc(&payload),
                signature: enc(&bytes[split..]),
            },
        })
    }

    /// The visible fields are only for display before scanning; the signed claim is what
    /// counts, so any disagreement is an error rather than silently preferring one.
    pub fn verify(&self, now: u64) -> Result<InviteClaim> {
        if self.v != 1 && self.v != 2 {
            return Err(Error::UnknownVersion);
        }
        let claim = open_invite(&self.invite)?;
        if claim.v != self.v {
            return Err(Error::TicketMismatch);
        }
        if claim.iss != self.did
            || claim.device != self.device
            || claim.name != self.name
            || claim.relay != self.relay
        {
            return Err(Error::TicketMismatch);
        }
        if now >= claim.exp {
            return Err(Error::Expired);
        }
        Ok(claim)
    }
}

pub fn issue_contact_ticket(
    id: &Identity,
    device: [u8; 32],
    name: &str,
    relay: Option<String>,
    now: u64,
    ttl_secs: u64,
) -> ContactTicket {
    // Fit the compact layout: name capped at a char boundary, expiry rounded up to a minute.
    let name = sanitize_name(name);
    let mut end = name.len().min(V2_MAX_NAME);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let name = &name[..end];
    let relay = relay.filter(|r| r.len() <= 255);
    let exp_min = now
        .saturating_add(ttl_secs)
        .saturating_sub(V2_EPOCH)
        .div_ceil(60)
        .min(0xFF_FFFF);
    let mut nonce = [0u8; V2_NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let code = match &relay {
        None => 0,
        Some(r) => KNOWN_RELAYS
            .iter()
            .position(|k| k == r)
            .map_or(RELAY_CUSTOM, |i| i as u8 + 1),
    };
    let mut payload = vec![V2_BYTE];
    payload.extend(id.signing_public_key());
    payload.extend(device);
    payload.extend(nonce);
    payload.extend(&exp_min.to_be_bytes()[5..]);
    payload.push(code << 5 | name.len() as u8);
    payload.extend(name.as_bytes());
    if code == RELAY_CUSTOM {
        let r = relay.as_deref().unwrap();
        payload.push(r.len() as u8);
        payload.extend(r.as_bytes());
    }
    let sig = id.sign(&[INVITE_V2_DOMAIN, &payload].concat());
    ContactTicket {
        v: 2,
        did: id.did().to_string(),
        name: name.to_string(),
        device: enc(device),
        relay,
        invite: SignedBlob {
            payload: enc(&payload),
            signature: enc(sig),
        },
    }
}

// ---- grant -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub v: u8,
    pub id: String,
    pub iss: String,
    pub holder: String,
    pub cap: String,
    pub iat: u64,
    pub exp: u64,
}

pub fn issue_grant(id: &Identity, holder_did: &str, now: u64, ttl: u64) -> SignedGrant {
    let claim = Grant {
        v: VERSION,
        id: random16(),
        iss: id.did().to_string(),
        holder: holder_did.to_string(),
        cap: CAP_CALL.to_string(),
        iat: now,
        exp: now.saturating_add(ttl),
    };
    sign_blob(id, GRANT_DOMAIN, &claim)
}

pub fn verify_grant(
    g: &SignedGrant,
    expected_iss: &str,
    expected_holder: &str,
    now: u64,
    revoked: &HashSet<String>,
) -> Result<Grant> {
    let grant: Grant = open_blob(g, GRANT_DOMAIN, |c: &Grant| &c.iss)?;
    check_version(grant.v)?;
    if grant.iss != expected_iss {
        return Err(Error::WrongIssuer);
    }
    if grant.holder != expected_holder {
        return Err(Error::WrongHolder);
    }
    if grant.cap != CAP_CALL {
        return Err(Error::BadCapability);
    }
    if now >= grant.exp {
        return Err(Error::Expired);
    }
    if revoked.contains(&grant.id) {
        return Err(Error::Revoked);
    }
    Ok(grant)
}

// ---- messages ----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    ContactHello {
        attestation: SignedAttestation,
        name: String,
        invite: SignedBlob,
        binding: String,
        grant_for_you: SignedGrant,
        /// The joiner's relay right now: an unsigned routing hint so the ticket owner can call
        /// back without waiting on DNS discovery. A wrong one only costs a slower dial.
        #[serde(default)]
        relay: Option<String>,
    },
    ContactWelcome {
        attestation: SignedAttestation,
        name: String,
        grant_for_you: SignedGrant,
    },
    Reject {
        reason: String,
    },
    CallHello {
        call_id: String,
        attestation: SignedAttestation,
        grant: SignedGrant,
        /// The caller's current relay, same kind of hint as in `ContactHello`.
        #[serde(default)]
        relay: Option<String>,
    },
    Ringing,
    Accept {
        renewed_grant: Option<SignedGrant>,
    },
    Decline {
        reason: String,
    },
    Busy,
    Hangup,
}

/// u32 BE length + JSON.
pub fn encode_frame(msg: &Msg) -> Vec<u8> {
    let json = serde_json::to_vec(msg).expect("msg serializes");
    let mut out = (json.len() as u32).to_be_bytes().to_vec();
    out.extend(json);
    out
}

/// `Ok(None)` = need more bytes. The length is checked before buffering the body so a peer
/// can't make us wait on (or allocate for) a huge frame.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Msg, usize)>> {
    let Some(head) = buf.first_chunk::<4>() else {
        return Ok(None);
    };
    let len = u32::from_be_bytes(*head) as usize;
    if len > MAX_FRAME {
        return Err(Error::FrameTooLarge);
    }
    let Some(body) = buf.get(4..4 + len) else {
        return Ok(None);
    };
    let msg = serde_json::from_slice(body).map_err(|_| Error::Decode)?;
    Ok(Some((msg, 4 + len)))
}

// ---- add-contact handshake ---------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PendingContact {
    pub did: String,
    pub name: String,
    pub device: [u8; 32],
    pub nonce: String,
    my_did: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewContact {
    pub did: String,
    pub name: String,
    pub device: [u8; 32],
    pub grant_from_them: SignedGrant,
    pub redeemed_nonce: String,
}

/// Binds the joiner's DID to *this* invite and *this* pair of devices, so a hello can't be
/// lifted onto another connection or invite.
fn binding_message(
    nonce: &str,
    joiner_device: &[u8; 32],
    owner_device: &[u8; 32],
) -> Result<Vec<u8>> {
    Ok([BIND_DOMAIN, &dec(nonce)?, joiner_device, owner_device].concat())
}

pub fn contact_hello(
    me: &Identity,
    my_device: [u8; 32],
    ticket: &ContactTicket,
    my_name: &str,
    now: u64,
    grant_ttl: u64,
) -> Result<(Msg, PendingContact)> {
    let claim = ticket.verify(now)?;
    if claim.iss == me.did() {
        return Err(Error::SelfContact);
    }
    let their_device = dec32(&claim.device)?;
    let binding = enc(me.sign(&binding_message(&claim.nonce, &my_device, &their_device)?));
    let msg = Msg::ContactHello {
        relay: None,
        attestation: attest(me, my_device, now),
        name: my_name.to_string(),
        invite: ticket.invite.clone(),
        binding,
        grant_for_you: issue_grant(me, &claim.iss, now, grant_ttl),
    };
    let pending = PendingContact {
        did: claim.iss,
        name: sanitize_name(&claim.name),
        device: their_device,
        nonce: claim.nonce,
        my_did: me.did().to_string(),
    };
    Ok((msg, pending))
}

pub fn accept_contact_hello(
    me: &Identity,
    my_device: [u8; 32],
    msg: &Msg,
    remote_device: [u8; 32],
    now: u64,
    redeemed_nonces: &HashSet<String>,
    grant_ttl: u64,
) -> Result<(Msg, NewContact)> {
    let Msg::ContactHello {
        attestation,
        name,
        invite,
        binding,
        grant_for_you,
        ..
    } = msg
    else {
        return Err(Error::UnexpectedMessage);
    };
    let claim = open_invite(invite)?;
    if claim.iss != me.did() {
        return Err(Error::WrongIssuer);
    }
    if now >= claim.exp {
        return Err(Error::Expired);
    }
    if dec32(&claim.device)? != my_device {
        return Err(Error::DeviceMismatch);
    }
    if redeemed_nonces.contains(&claim.nonce) {
        return Err(Error::Replayed);
    }
    let att = verify_attestation(attestation)?;
    if att.device_key()? != remote_device {
        return Err(Error::DeviceMismatch);
    }
    let their_pub = public_key_from_did(&att.did).ok_or(Error::Decode)?;
    let sig: [u8; 64] = dec(binding)?.try_into().map_err(|_| Error::Decode)?;
    let bind_msg = binding_message(&claim.nonce, &remote_device, &my_device)?;
    if !verify(&their_pub, &bind_msg, &sig) {
        return Err(Error::BadSignature);
    }
    verify_grant(grant_for_you, &att.did, me.did(), now, &HashSet::new())?;

    let welcome = Msg::ContactWelcome {
        attestation: attest(me, my_device, now),
        name: sanitize_name(&claim.name),
        grant_for_you: issue_grant(me, &att.did, now, grant_ttl),
    };
    let contact = NewContact {
        did: att.did,
        name: sanitize_name(name),
        device: remote_device,
        grant_from_them: grant_for_you.clone(),
        redeemed_nonce: claim.nonce,
    };
    Ok((welcome, contact))
}

pub fn accept_contact_welcome(
    me: &Identity,
    pending: &PendingContact,
    msg: &Msg,
    remote_device: [u8; 32],
    now: u64,
) -> Result<NewContact> {
    let Msg::ContactWelcome {
        attestation,
        grant_for_you,
        ..
    } = msg
    else {
        return Err(Error::UnexpectedMessage);
    };
    if pending.my_did != me.did() {
        return Err(Error::WrongHolder);
    }
    let att = verify_attestation(attestation)?;
    if att.did != pending.did {
        return Err(Error::WrongIssuer);
    }
    let dev = att.device_key()?;
    if dev != remote_device || dev != pending.device {
        return Err(Error::DeviceMismatch);
    }
    verify_grant(grant_for_you, &pending.did, me.did(), now, &HashSet::new())?;
    Ok(NewContact {
        did: pending.did.clone(),
        // The ticket's signed name, not the welcome's unsigned one.
        name: pending.name.clone(),
        device: dev,
        grant_from_them: grant_for_you.clone(),
        redeemed_nonce: pending.nonce.clone(),
    })
}

// ---- calls -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub did: String,
    pub device: [u8; 32],
    pub grant_id: String,
}

/// `_me` is unused today (the attestation and grant are already signed); kept so the call
/// site mirrors `accept_call_hello` and a future per-call signature needs no API break.
pub fn call_hello(
    _me: &Identity,
    my_device_attestation: SignedAttestation,
    grant_from_them: SignedGrant,
    call_id: String,
) -> Msg {
    Msg::CallHello {
        call_id,
        attestation: my_device_attestation,
        grant: grant_from_them,
        relay: None,
    }
}

pub fn accept_call_hello(
    me: &Identity,
    msg: &Msg,
    remote_device: [u8; 32],
    now: u64,
    revoked_grant_ids: &HashSet<String>,
    is_contact: impl Fn(&str) -> bool,
) -> Result<Caller> {
    let Msg::CallHello {
        attestation, grant, ..
    } = msg
    else {
        return Err(Error::UnexpectedMessage);
    };
    let att = verify_attestation(attestation)?;
    if att.device_key()? != remote_device {
        return Err(Error::DeviceMismatch);
    }
    // Holder is the attested DID, so a grant lifted by someone else fails here.
    let g = verify_grant(grant, me.did(), &att.did, now, revoked_grant_ids)?;
    if !is_contact(&att.did) {
        return Err(Error::NotContact);
    }
    Ok(Caller {
        did: att.did,
        device: remote_device,
        grant_id: g.id,
    })
}

#[cfg(test)]
mod tests;
