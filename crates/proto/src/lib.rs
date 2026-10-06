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
const BIND_DOMAIN: &[u8] = b"osvauld/p2p/bind/v1\0";

const VERSION: u8 = 1;
const TICKET_PREFIX: &str = "osvc1.";
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

/// No expiry: it asserts a fact about key custody, and revoking a device is done by
/// revoking grants. `iat` is informational (callers may age it out).
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
    let claim: InviteClaim = open_blob(blob, INVITE_DOMAIN, |c: &InviteClaim| &c.iss)?;
    check_version(claim.v)?;
    if dec(&claim.nonce)?.len() != 16 {
        return Err(Error::Decode);
    }
    Ok(claim)
}

impl ContactTicket {
    /// Same reasoning as courier's tickets: a distinct prefix makes the kind obvious on sight.
    pub fn to_text(&self) -> String {
        let json = serde_json::to_vec(self).expect("ticket serializes");
        format!("{TICKET_PREFIX}{}", enc(json))
    }

    pub fn from_text(text: &str) -> Result<Self> {
        let body = text
            .trim()
            .strip_prefix(TICKET_PREFIX)
            .ok_or(Error::UnknownVersion)?;
        let t: Self = serde_json::from_slice(&dec(body)?).map_err(|_| Error::Decode)?;
        check_version(t.v)?;
        Ok(t)
    }

    /// The visible fields are only for display before scanning; the signed claim is what
    /// counts, so any disagreement is an error rather than silently preferring one.
    pub fn verify(&self, now: u64) -> Result<InviteClaim> {
        check_version(self.v)?;
        let claim = open_invite(&self.invite)?;
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
    let claim = InviteClaim {
        v: VERSION,
        iss: id.did().to_string(),
        nonce: random16(),
        iat: now,
        exp: now.saturating_add(ttl_secs),
        device: enc(device),
        name: name.to_string(),
        relay: relay.clone(),
    };
    ContactTicket {
        v: VERSION,
        did: claim.iss.clone(),
        name: name.to_string(),
        device: claim.device.clone(),
        relay,
        invite: sign_blob(id, INVITE_DOMAIN, &claim),
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
        attestation: attest(me, my_device, now),
        name: my_name.to_string(),
        invite: ticket.invite.clone(),
        binding,
        grant_for_you: issue_grant(me, &claim.iss, now, grant_ttl),
    };
    let pending = PendingContact {
        did: claim.iss,
        name: claim.name,
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
        name: claim.name,
        grant_for_you: issue_grant(me, &att.did, now, grant_ttl),
    };
    let contact = NewContact {
        did: att.did,
        name: name.clone(),
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
