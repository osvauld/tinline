//! Shared message/call receipt codec, Loro primitives and transactional ledger, with no IO
//! runtime, transport or UniFFI (p2pcore re-exports it as `p2pcore::receipts` and owns the
//! app-facing API and tick visuals).
//! No runtime lifecycle/content issuance, admission policy, wire or UI hooks yet.
//! A signature is a claim, not a disk/read audit. Host/cache proofs are separate.
use crdt::{day_of, day_start};
use cryptography::{domains, hash};
use proto::{Signable, Signed, Signer};
use serde::{Deserialize, Serialize};

pub mod calls;
pub mod doc;
pub mod evidence;
// Group-call join proofs and the evidence bundles are built and tested but not yet wired into a
// runtime; they compile for tests only until the group receipt wire lands.
#[cfg(test)]
pub mod participation;
pub mod store;
/// Shared test setup (in-memory/temporary ledgers); p2pcore's tests use it via `testkit`.
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
// Provisional budgets indexed in docs/design/sync-receipt-limits.md.
pub const MAX_MESSAGES: usize = 256;
pub const MAX_CONTEXT_WRITERS: usize = 1024;
pub const MAX_BODY: usize = 32 * 1024;
#[cfg(test)]
pub const MAX_SIGNED: usize = MAX_BODY + proto::signed::SIG_LEN;
pub const MAX_UPDATE_BYTES: usize = crdt::batching::MAX_EXPORT_BYTES;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Conversation {
    Direct(String),
    Group(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
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
pub enum Kind {
    Delivered,
    Read,
    Call,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
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
pub type SignedReceipt = Signed<Receipt>;

impl Signable for Receipt {
    const DOMAIN: domains::Domain = domains::RECEIPT;
    const MAX_BODY: usize = MAX_BODY;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.recipient)
    }
    fn check(&self) -> proto::Result<()> {
        self.check_shape().map_err(|_| proto::Error::Decode)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReceiptError {
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
#[cfg(test)]
pub fn message_digest(original: &[u8]) -> [u8; 32] {
    hash::content(domains::RECEIPT_MESSAGE, original)
}
pub fn receipt_id(original: &[u8]) -> String {
    hash::hasher(domains::RECEIPT_ID).part(original).hex()
}
impl Receipt {
    fn check_shape(&self) -> Result<(), ReceiptError> {
        if !self.scope.valid()
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
    pub fn sign_claim(
        &self,
        account: &identity::Identity,
    ) -> Result<SignedReceipt, ReceiptError> {
        self.check_shape()?;
        Signed::sign(self, account).map_err(|e| match e {
            proto::Error::WrongIssuer => ReceiptError::Signature,
            _ => ReceiptError::Bounds,
        })
    }
}
/// Cryptographic check only. Membership/device/contact/content coverage are
/// caller policy; a valid signature alone must not advance delivery/read UI.
pub fn open_receipt(bytes: &[u8], scope: &Scope) -> Result<Receipt, ReceiptError> {
    let signed = SignedReceipt::from_bytes(bytes).map_err(|_| ReceiptError::Bounds)?;
    let claim = signed.peek().map_err(|_| ReceiptError::Encoding)?;
    claim.check_shape()?;
    if &claim.scope != scope {
        return Err(ReceiptError::Scope);
    }
    signed.open().map_err(|e| match e {
        proto::Error::BadSignature => ReceiptError::Signature,
        _ => ReceiptError::Encoding,
    })
}
/// `open_receipt` without the signature check, for statements already verified on arrival
/// (bookkeeping over a committed ledger, never admission).
pub(crate) fn peek_receipt(bytes: &[u8], scope: &Scope) -> Result<Receipt, ReceiptError> {
    let claim = SignedReceipt::from_bytes(bytes)
        .map_err(|_| ReceiptError::Bounds)?
        .peek()
        .map_err(|_| ReceiptError::Encoding)?;
    if &claim.scope != scope {
        return Err(ReceiptError::Scope);
    }
    Ok(claim)
}
#[cfg(test)]
mod tests;
