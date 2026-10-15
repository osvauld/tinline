//! Exact original signed group-call protocol evidence (a `proto::Signed` envelope's bytes);
//! no endpoint notes/media.
use super::*;
pub(crate) const MAX_PROTOCOL_EVIDENCE_BYTES: usize =
    proto::gcall::MAX_CONTROL + proto::signed::SIG_LEN;
#[derive(Debug, Clone)]
pub(crate) struct Evidence {
    pub id: [u8; 32],
    pub bytes: Vec<u8>,
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    cryptography::hash::content(cryptography::domains::RECEIPT_EVIDENCE, bytes)
}
impl Evidence {
    /// Size only; the caller must open the actual protocol statement and validate its scope,
    /// roles and lifecycle correspondence.
    pub(crate) fn from_blob<T: Signable>(blob: &Signed<T>) -> Result<Self, ReceiptError> {
        let bytes = blob.to_bytes();
        if bytes.len() > MAX_PROTOCOL_EVIDENCE_BYTES {
            return Err(ReceiptError::Bounds);
        }
        Ok(Self {
            id: hash(&bytes),
            bytes,
        })
    }
    /// Evidence from stored envelope bytes.
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Result<Self, ReceiptError> {
        let e = Self {
            id: hash(&bytes),
            bytes,
        };
        e.check()?;
        Ok(e)
    }
    /// Size and id only.
    pub(crate) fn check(&self) -> Result<(), ReceiptError> {
        if self.bytes.len() > MAX_PROTOCOL_EVIDENCE_BYTES
            || self.bytes.len() < proto::signed::SIG_LEN
            || hash(&self.bytes) != self.id
        {
            return Err(ReceiptError::Bounds);
        }
        Ok(())
    }
    pub(crate) fn blob<T: Signable>(&self) -> Result<Signed<T>, ReceiptError> {
        self.check()?;
        Signed::from_bytes(&self.bytes).map_err(|_| ReceiptError::Encoding)
    }
}
