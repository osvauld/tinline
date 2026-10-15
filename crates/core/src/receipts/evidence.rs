//! Exact original signed group-call protocol evidence; no endpoint notes/media.
use super::*;
const SIG_TEXT_BYTES: usize = (SIGNATURE_BYTES * 8).div_ceil(6); // unpadded base64url
pub(crate) const MAX_PROTOCOL_EVIDENCE_BYTES: usize =
    2 * proto::gcall::MAX_CONTROL + 2 * codec::LEN_PREFIX_MAX + SIG_TEXT_BYTES;
#[derive(Debug, Clone)]
pub(crate) struct Evidence {
    pub id: [u8; 32],
    pub bytes: Vec<u8>,
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    cryptography::hash::content(cryptography::domains::RECEIPT_EVIDENCE, bytes)
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
        let bytes = codec::to_bytes(blob).map_err(|_| ReceiptError::Encoding)?;
        Ok(Self {
            id: hash(&bytes),
            bytes,
        })
    }
    pub(crate) fn blob(&self) -> Result<proto::SignedBlob, ReceiptError> {
        if self.bytes.len() > MAX_PROTOCOL_EVIDENCE_BYTES || hash(&self.bytes) != self.id {
            return Err(ReceiptError::Bounds);
        }
        let blob: proto::SignedBlob = codec::from_bytes_canonical(&self.bytes, MAX_PROTOCOL_EVIDENCE_BYTES)
            .map_err(|_| ReceiptError::Encoding)?;
        Self::from_blob(&blob)?; // the same bounds as on the way in
        Ok(blob)
    }
}
