//! Shared message/call receipt codec, Loro primitives and transactional ledger.
//! No runtime lifecycle/content issuance, admission policy, wire or UI hooks yet.
//! A signature is a claim, not a disk/read audit. Host/cache proofs are separate.
use crate::{
    chat::doc::{day_of, day_start},
    group::record::{put_bytes_pub, verify_did_pub},
};
use bincode::Options;
use serde::{Deserialize, Serialize};

pub(crate) mod calls;
pub(crate) mod doc;
pub(crate) mod evidence;
pub(crate) mod participation;
pub(crate) mod store;
const DOMAIN: &[u8] = b"tinline-recipient-receipt-v1\0";
const MESSAGE_DOMAIN: &[u8] = b"tinline-receipt-original-message-v1\0";
const RECEIPT_ID_DOMAIN: &[u8] = b"tinline-recipient-receipt-id-v1\0";
// Provisional budgets indexed in docs/design/sync-receipt-limits.md.
pub(crate) const MAX_MESSAGES: usize = 256;
pub(crate) const MAX_CONTEXT_WRITERS: usize = 1024;
pub(crate) const MAX_BODY: usize = 32 * 1024;
const SIGNATURE_BYTES: usize = 64;
const VEC_LENGTH_BYTES: usize = 8;
pub(crate) const MAX_SIGNED: usize = MAX_BODY + 2 * VEC_LENGTH_BYTES + SIGNATURE_BYTES;
pub(crate) const MAX_UPDATE_BYTES: usize = crate::group::doc::batching::MAX_EXPORT_BYTES;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Conversation {
    Direct(String),
    Group(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Scope {
    pub conversation: Conversation,
    pub day: String,
}
impl Scope {
    fn valid(&self) -> bool {
        let id = match &self.conversation {
            Conversation::Direct(id) | Conversation::Group(id) => id,
        };
        id.len() == 64
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && day_start(&self.day).is_some_and(|ms| day_of(ms) == self.day)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    Delivered,
    Read,
    Call,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub version: u8,
    pub scope: Scope,
    pub kind: Kind,
    pub recipient: String,
    /// Scoped per-install ID, never a network endpoint address.
    pub device: [u8; 32],
    /// Exact sorted distinct original signed message/version digests. No implicit
    /// read cursor and no received-history VV: unread holes remain holes.
    pub messages: Vec<[u8; 32]>,
    /// Group metadata context, not authority or trusted signing time. Direct chat
    /// has no group metadata and must use an empty context. Admission is separate.
    pub metadata: Vec<(u64, i32)>,
    /// Typed local/session observation; call claims must have no message hashes.
    /// This extends the UNDEPLOYED prototype schema, not legacy stored/wire data.
    pub call: Option<calls::CallEvent>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SignedReceipt {
    pub body: Vec<u8>,
    pub sig: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ReceiptError {
    #[error("recipient receipt bounds or shape")]
    Bounds,
    #[error("recipient receipt encoding")]
    Encoding,
    #[error("recipient receipt scope mismatch")]
    Scope,
    #[error("recipient receipt signature")]
    Signature,
    #[error("recipient receipt native structure")]
    Structure,
    #[error("recipient receipt key mismatch")]
    Key,
    #[error("recipient receipt writer mismatch")]
    Writer,
    #[error("recipient receipt pending predecessors")]
    Pending,
    #[error("recipient receipt policy: {0}")]
    Policy(String),
    #[error("recipient receipt store: {0}")]
    Store(String),
}
/// Low-level hash of canonical ORIGINAL SIGNED message/version bytes, not text
/// or a random record ID. Callers must validate/canonicalize those bytes first.
/// The real DM/group extraction adapters are not implemented by this helper.
pub(crate) fn message_digest(original: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(MESSAGE_DOMAIN);
    h.update(&(original.len() as u64).to_be_bytes());
    h.update(original);
    *h.finalize().as_bytes()
}
pub(crate) fn receipt_id(original: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(RECEIPT_ID_DOMAIN);
    h.update(&(original.len() as u64).to_be_bytes());
    h.update(original);
    h.finalize().to_hex().to_string()
}
fn sign_input(body: &[u8]) -> Vec<u8> {
    let mut b = DOMAIN.to_vec();
    put_bytes_pub(&mut b, body);
    b
}
impl Receipt {
    fn check(&self) -> Result<(), ReceiptError> {
        if self.version != 1
            || !self.scope.valid()
            || self.recipient.len() > 200
            || identity::public_key_from_did(&self.recipient).is_none()
            || self.messages.len() > MAX_MESSAGES
            || !self.messages.windows(2).all(|w| w[0] < w[1])
            || self.metadata.len() > MAX_CONTEXT_WRITERS
            || !self.metadata.iter().all(|(_, end)| *end > 0)
            || !self.metadata.windows(2).all(|w| w[0].0 < w[1].0)
            || match self.scope.conversation {
                Conversation::Direct(_) => !self.metadata.is_empty(),
                Conversation::Group(_) => self.metadata.is_empty(),
            }
        {
            return Err(ReceiptError::Bounds);
        }
        match self.kind {
            Kind::Delivered | Kind::Read if self.messages.is_empty() || self.call.is_some() => {
                return Err(ReceiptError::Bounds);
            }
            Kind::Call => {
                if !self.messages.is_empty() {
                    return Err(ReceiptError::Bounds);
                }
                self.call
                    .as_ref()
                    .ok_or(ReceiptError::Bounds)?
                    .check(&self.scope)?;
            }
            _ => {}
        }
        Ok(())
    }
    /// Low-level claim signer ONLY. A future runtime controller must derive
    /// coverage from durable verified messages/read events, not peer assertions.
    pub(crate) fn sign_claim(
        &self,
        account: &identity::Identity,
    ) -> Result<SignedReceipt, ReceiptError> {
        self.check()?;
        if account.did() != self.recipient {
            return Err(ReceiptError::Signature);
        }
        let body = bincode::serialize(self).map_err(|_| ReceiptError::Encoding)?;
        if body.len() > MAX_BODY {
            return Err(ReceiptError::Bounds);
        }
        Ok(SignedReceipt {
            sig: account.sign(&sign_input(&body)).to_vec(),
            body,
        })
    }
}
impl SignedReceipt {
    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, ReceiptError> {
        if self.body.len() > MAX_BODY || self.sig.len() != SIGNATURE_BYTES {
            return Err(ReceiptError::Bounds);
        }
        bincode::serialize(self).map_err(|_| ReceiptError::Encoding)
    }
    /// Cryptographic check only. Membership/device/contact/content coverage are
    /// caller policy; a valid signature alone must not advance delivery/read UI.
    pub(crate) fn open(bytes: &[u8], scope: &Scope) -> Result<Receipt, ReceiptError> {
        if bytes.len() > MAX_SIGNED {
            return Err(ReceiptError::Bounds);
        }
        let signed: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_SIGNED as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)
            .map_err(|_| ReceiptError::Encoding)?;
        if signed.to_bytes()?.as_slice() != bytes {
            return Err(ReceiptError::Encoding);
        }
        let claim: Receipt = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_BODY as u64)
            .reject_trailing_bytes()
            .deserialize(&signed.body)
            .map_err(|_| ReceiptError::Encoding)?;
        if bincode::serialize(&claim).map_err(|_| ReceiptError::Encoding)? != signed.body {
            return Err(ReceiptError::Encoding);
        }
        claim.check()?;
        if &claim.scope != scope {
            return Err(ReceiptError::Scope);
        }
        if !verify_did_pub(&claim.recipient, &sign_input(&signed.body), &signed.sig) {
            return Err(ReceiptError::Signature);
        }
        Ok(claim)
    }
}
#[cfg(test)]
mod tests;
