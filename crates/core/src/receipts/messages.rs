//! The receipt digest of a DM message version. It reads the chat record (`chat::doc::MsgRec`),
//! so it stays in p2pcore; the issuance ledger it feeds is `receipts_core::store::messages`.
use crate::chat::doc::MsgRec;

/// Canonical identity of ONE persisted, verified DM message version: the doc,
/// record id, author, writing install and the author's DID signature, which was
/// verified over the full version content and its exact op span before the
/// version was stored (`chat::doc::apply_remote`/local write). An edit or delete
/// is a new signed version and therefore a new digest: receipts never carry over.
pub(crate) fn dm_version(doc: &str, rec: &MsgRec) -> [u8; 32] {
    cryptography::hash::hasher(cryptography::domains::RECEIPT_DM_VERSION)
        .part(doc.as_bytes())
        .part(rec.id.as_bytes())
        .part(rec.author.as_bytes())
        .part(&rec.dev)
        .part(&rec.did_sig)
        .finish()
}

#[cfg(test)]
mod tests;
