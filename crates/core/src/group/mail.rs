//! Sealed mail between two members through a group host (U3: 1:1 chat requests).
//!
//! A `Mail` envelope names the group, the sender and recipient DIDs, a random id and the time,
//! and carries `sealed`: ECIES (`identity::encrypt_for`) to the recipient's encryption key, which
//! every member publishes DID-signed in the group (`Body::EncKey`). The envelope is DID-signed by
//! the sender, so a host checks who sends to whom (members only, not removed) and relays it, but
//! cannot read it. Inside, the `Letter` repeats group, sender and recipient, so a sealed body
//! cannot be lifted into another envelope.
//!
//! ```text
//! requester -> host -> recipient   Request { req, name }
//! recipient -> host -> requester   Accept { req, ticket }   (one-time contact ticket of the
//!                                                            recipient's PRIVATE endpoint)
//!                                | Decline { req, blocked }
//! ```
//! On `Accept` the requester adds the recipient from the ticket (`ContactOrigin::GroupRequest`:
//! message token only); the chat then runs directly between private endpoints, without the host.

use serde::{Deserialize, Serialize};

use super::record::{put_bytes_pub as put_bytes, verify_did_pub as verify_did};

const MAIL_DOMAIN: &[u8] = b"tinline-group-mail-v1";
/// Largest sealed body (a contact ticket and a name fit easily).
pub const MAX_SEALED: usize = 8 * 1024;
/// How long hosts keep undelivered mail and recipients remember mail ids (PROVISIONAL).
pub const MAIL_TTL_MS: u64 = 7 * 24 * 3600 * 1000;
/// Undelivered mail a host keeps per recipient (PROVISIONAL).
pub const MAX_PENDING_PER_RECIPIENT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mail {
    pub group: String,
    pub from: String,
    pub to: String,
    pub id: [u8; 16],
    /// Unix ms, the sender's clock.
    pub at: u64,
    pub sealed: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedMail {
    pub body: Vec<u8>,
    pub sig: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Letter {
    /// "May we chat 1:1?" `name`: how the requester calls itself.
    Request { req: [u8; 16], name: String },
    /// Yes: a one-time contact ticket for the recipient's private endpoint.
    Accept { req: [u8; 16], ticket: String },
    /// No (`blocked`: the recipient blocks the requester, P5).
    Decline { req: [u8; 16], blocked: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Sealed {
    group: String,
    from: String,
    to: String,
    letter: Letter,
}

fn sig_input(body: &[u8]) -> Vec<u8> {
    let mut v = MAIL_DOMAIN.to_vec();
    put_bytes(&mut v, body);
    v
}

impl SignedMail {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("mail serialises")
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() > MAX_SEALED + 2048 {
            return None;
        }
        let s: SignedMail = bincode::deserialize(b).ok()?;
        (s.to_bytes() == b).then_some(s)
    }

    /// The envelope, if its sender's DID signed it and it is well formed.
    pub fn open(&self) -> Option<Mail> {
        let m: Mail = bincode::deserialize(&self.body).ok()?;
        if bincode::serialize(&m).ok()? != self.body || m.sealed.len() > MAX_SEALED || m.from.len() > 200 || m.to.len() > 200 || m.from == m.to {
            return None;
        }
        verify_did(&m.from, &sig_input(&self.body), &self.sig).then_some(m)
    }
}

pub fn id_hex(id: &[u8; 16]) -> String {
    hex::encode(id)
}

/// Seals `letter` from `account` to `to` (whose encryption key is `to_key`) for `group`.
pub fn seal(account: &identity::Identity, group: &str, to: &str, to_key: &[u8; 32], letter: Letter, at: u64) -> Result<SignedMail, crate::Error> {
    let inner = Sealed { group: group.to_string(), from: account.did().to_string(), to: to.to_string(), letter };
    let plain = bincode::serialize(&inner).expect("letter serialises");
    let sealed = identity::encrypt_for(to_key, &plain).map_err(|e| crate::Error::Protocol(e.to_string()))?;
    let mail = Mail { group: group.to_string(), from: account.did().to_string(), to: to.to_string(), id: cryptography::rng::bytes(), at, sealed };
    let body = bincode::serialize(&mail).expect("mail serialises");
    let sig = account.sign(&sig_input(&body)).to_vec();
    Ok(SignedMail { body, sig })
}

/// Opens mail addressed to `account` in `group`: the envelope verifies, it is for us, the body
/// decrypts with our key and repeats the envelope's group, sender and recipient.
pub fn open_for(account: &identity::Identity, group: &str, s: &SignedMail) -> Option<(Mail, Letter)> {
    let m = s.open()?;
    if m.group != group || m.to != account.did() {
        return None;
    }
    let plain = account.decrypt_sealed(&m.sealed).ok()?;
    let inner: Sealed = bincode::deserialize(&plain).ok()?;
    (inner.group == m.group && inner.from == m.from && inner.to == m.to).then_some((m, inner.letter))
}

#[cfg(test)]
mod tests;
