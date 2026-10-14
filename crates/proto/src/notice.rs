//! Signed block notices.

use super::*;

// ---- block notices ------------------------------------------------------------------

/// "`from` blocked `to`" (or, with `blocked: false`, lifted that block). Signed by `from`'s DID
/// key, never by a device key, so every device of `from` hands out the same notice and nobody
/// re-signs it per device. A block notice is also a deletion request (U20): the receiving app
/// deletes its contact entry for `from` and keeps the chat history read-only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockNotice {
    pub v: u8,
    pub from: String,
    pub to: String,
    pub blocked: bool,
    /// Unix ms chosen by the signer, strictly increasing per (`from`, `to`): a receiver keeps
    /// only the notice with the highest `at`, so a replayed older one changes nothing.
    pub at: u64,
}

pub fn sign_block_notice(id: &Identity, to: &str, blocked: bool, at: u64) -> SignedBlob {
    let claim = BlockNotice {
        v: VERSION,
        from: id.did().to_string(),
        to: to.to_string(),
        blocked,
        at,
    };
    sign_blob(id, BLOCK_DOMAIN, &claim)
}

/// Checks the signature under the DID the notice names as `from`, and that it is addressed to
/// `expected_to`. Whether it is newer than one already held is the caller's check.
pub fn verify_block_notice(blob: &SignedBlob, expected_to: &str) -> Result<BlockNotice> {
    let n: BlockNotice = open_blob(blob, BLOCK_DOMAIN, |c: &BlockNotice| &c.from)?;
    check_version(n.v)?;
    if n.to != expected_to {
        return Err(Error::WrongHolder);
    }
    if n.from == n.to {
        return Err(Error::SelfContact);
    }
    Ok(n)
}

/// For tests only: a notice that claims to come from `claimed_from` but is signed by `signer`
/// (a forgery unless `signer` is that DID).
#[doc(hidden)]
pub fn forge_block_notice_for_test(
    signer: &Identity,
    claimed_from: &str,
    to: &str,
    blocked: bool,
    at: u64,
) -> SignedBlob {
    let claim = BlockNotice {
        v: VERSION,
        from: claimed_from.to_string(),
        to: to.to_string(),
        blocked,
        at,
    };
    sign_blob(signer, BLOCK_DOMAIN, &claim)
}
