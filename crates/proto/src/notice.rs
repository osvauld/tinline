//! Signed block notices.

use super::*;

// ---- block notices ------------------------------------------------------------------

/// "`from` blocked `to`" (or, with `blocked: false`, lifted that block). Signed by `from`'s DID
/// key, never by a device key, so every device of `from` hands out the same notice and nobody
/// re-signs it per device. A block notice is also a deletion request (U20): the receiving app
/// deletes its contact entry for `from` and keeps the chat history read-only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockNotice {
    pub from: String,
    pub to: String,
    pub blocked: bool,
    /// Unix ms chosen by the signer, strictly increasing per (`from`, `to`): a receiver keeps
    /// only the notice with the highest `at`, so a replayed older one changes nothing.
    pub at: u64,
}

impl Signable for BlockNotice {
    const DOMAIN: domains::Domain = domains::BLOCK;
    const MAX_BODY: usize = 256;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.from)
    }
    fn check(&self) -> Result<()> {
        if self.from == self.to {
            return Err(Error::SelfContact);
        }
        Ok(())
    }
}

pub type SignedBlockNotice = Signed<BlockNotice>;

pub fn sign_block_notice(id: &Identity, to: &str, blocked: bool, at: u64) -> SignedBlockNotice {
    let claim = BlockNotice {
        from: id.did().to_string(),
        to: to.to_string(),
        blocked,
        at,
    };
    Signed::sign(&claim, id).expect("own notice signs")
}

/// Checks the signature under the DID the notice names as `from`, and that it is addressed to
/// `expected_to`. Whether it is newer than one already held is the caller's check.
pub fn verify_block_notice(blob: &SignedBlockNotice, expected_to: &str) -> Result<BlockNotice> {
    let n = blob.open()?;
    if n.to != expected_to {
        return Err(Error::WrongHolder);
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
) -> SignedBlockNotice {
    let claim = BlockNotice {
        from: claimed_from.to_string(),
        to: to.to_string(),
        blocked,
        at,
    };
    let body = cryptography::codec::to_bytes(&claim).expect("notice encodes");
    let sig = signed::sign_detached(signer, domains::BLOCK, &[&body]);
    Signed::from_parts(body, sig)
}
