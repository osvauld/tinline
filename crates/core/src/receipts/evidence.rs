//! Exact original signed group-call protocol evidence; no endpoint notes/media.
use super::*;
const DOMAIN: &[u8] = b"tinline-receipt-protocol-evidence-v1\0";
const SIG_TEXT_BYTES: usize = (SIGNATURE_BYTES * 8).div_ceil(6); // unpadded base64url
pub(crate) const MAX_PROTOCOL_EVIDENCE_BYTES: usize =
    2 * proto::gcall::MAX_CONTROL + 2 * VEC_LENGTH_BYTES + SIG_TEXT_BYTES;
#[derive(Debug, Clone)]
pub(crate) struct Evidence {
    pub id: [u8; 32],
    pub bytes: Vec<u8>,
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(DOMAIN);
    h.update(&(bytes.len() as u64).to_be_bytes());
    h.update(bytes);
    *h.finalize().as_bytes()
}
impl Evidence {
    /// Canonical envelope/size only; the caller must open the actual protocol
    /// statement and validate its scope, roles and lifecycle correspondence.
    pub(crate) fn from_blob(blob: &proto::SignedBlob) -> Result<Self, ReceiptError> {
        if blob.payload.len() > 2 * proto::gcall::MAX_CONTROL
            || blob.signature.len() != SIG_TEXT_BYTES
        {
            return Err(ReceiptError::Bounds);
        }
        let bytes = bincode::serialize(blob).map_err(|_| ReceiptError::Encoding)?;
        Ok(Self {
            id: hash(&bytes),
            bytes,
        })
    }
    pub(crate) fn blob(&self) -> Result<proto::SignedBlob, ReceiptError> {
        if self.bytes.len() > MAX_PROTOCOL_EVIDENCE_BYTES || hash(&self.bytes) != self.id {
            return Err(ReceiptError::Bounds);
        }
        let blob: proto::SignedBlob = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_PROTOCOL_EVIDENCE_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(&self.bytes)
            .map_err(|_| ReceiptError::Encoding)?;
        if Self::from_blob(&blob)?.bytes != self.bytes {
            return Err(ReceiptError::Encoding);
        }
        Ok(blob)
    }
}
