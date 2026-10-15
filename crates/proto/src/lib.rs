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

pub mod profile;
pub mod signed;

pub use signed::{SecretKey, SignKey, Signable, Signed, Signer, SignerId};

use std::collections::HashSet;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cryptography::{domains, sign};
use identity::{Identity, public_key_from_did, verify};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Calls, contact exchange and notices. Version 1 (P6): separate `message` and `call` tokens;
/// version 0 (one `call` grant for everything) is refused by the app.
pub const ALPN: &[u8] = b"osvauld/p2p/1";
/// Own-device sync (docs/design/device-linking.md §4); only devices of the same DID. Version 3
/// (P10 per-writer chat versions; P6: the account doc carries call tokens, call permissions and
/// token cut-offs); versions 2 and 1 are refused by the app.
pub const SELF_ALPN: &[u8] = b"tinline/self/5";

const ATTEST_DOMAIN: domains::Domain = domains::ATTEST;
const INVITE_DOMAIN: domains::Domain = domains::INVITE;
const GRANT_DOMAIN: domains::Domain = domains::GRANT;
const INVITE_V2_DOMAIN: domains::Domain = domains::INVITE_V2;
const BIND_DOMAIN: domains::Domain = domains::BIND;
const DEVICES_DOMAIN: domains::Domain = domains::DEVICES;
const BLOCK_DOMAIN: domains::Domain = domains::BLOCK;

const VERSION: u8 = 1;
const TICKET_PREFIX: &str = "osvc1.";
const TICKET_PREFIX_V2: &str = "OSVC2:";
pub const CAP_MESSAGE: &str = "message";
pub const CAP_CALL: &str = "call";
/// Tokens (`Grant`) we issue now carry exactly one capability. Version 1 grants are the legacy
/// single `call` grant, which counts as message + call (U13 migration).
const GRANT_VERSION: u8 = 2;
const LEGACY_GRANT_VERSION: u8 = 1;
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
    #[error("too many devices")]
    TooManyDevices,
    #[error("device was removed from the account")]
    Tombstoned,
    #[error("signed body too large")]
    TooLarge,
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
    cryptography::rng::fill(&mut b);
    enc(b)
}

// ---- device keys -------------------------------------------------------------------

/// Device key text form is `enc(32 bytes)`; the secret's storage is the caller's job.
pub fn new_device_secret() -> [u8; 32] {
    let mut s = [0u8; 32];
    cryptography::rng::fill(&mut s);
    s
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

/// Shared display-name character policy, including profile fields.
pub fn safe_name_char(c: char) -> bool {
    !c.is_control()
        && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}')
}

/// A name from a peer, made safe to show: control characters and bidi overrides/isolates (which
/// can reorder the text around them) are dropped, and the rest is cut to `MAX_NAME_CHARS`.
pub fn sanitize_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| safe_name_char(*c))
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

fn sign_blob<T: Serialize>(id: &Identity, domain: domains::Domain, claim: &T) -> SignedBlob {
    let payload = serde_json::to_vec(claim).expect("claims serialize");
    SignedBlob {
        signature: enc(id.sign(&sign::input(domain, &[&payload]))),
        payload: enc(payload),
    }
}

/// The signer is whoever the payload *claims* (`issuer_of`); the key comes from that DID, so
/// a valid signature proves the claimed DID wrote it. Callers then check it is the DID they
/// expected.
fn open_blob<T: DeserializeOwned>(
    blob: &SignedBlob,
    domain: domains::Domain,
    issuer_of: impl Fn(&T) -> &str,
) -> Result<T> {
    let payload = dec(&blob.payload)?;
    let sig: [u8; 64] = dec(&blob.signature)?
        .try_into()
        .map_err(|_| Error::Decode)?;
    let claim: T = serde_json::from_slice(&payload).map_err(|_| Error::Decode)?;
    let public = public_key_from_did(issuer_of(&claim)).ok_or(Error::Decode)?;
    if !verify(&public, &sign::input(domain, &[&payload]), &sig) {
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

mod call;
mod devlist;
mod grant;
mod handshake;
mod msg;
mod notice;
mod ticket;

pub use call::*;
pub use devlist::*;
pub use grant::*;
pub use handshake::*;
pub use msg::*;
pub use notice::*;
pub use ticket::*;

pub mod link;
pub use link::*;

/// Group calls through one call host (docs/design/group-call-spec.md, G6).
pub mod gcall;
/// Signed call-host conditions (group-call spec §2.2).
pub mod gcall_status;

#[cfg(test)]
mod tests;
