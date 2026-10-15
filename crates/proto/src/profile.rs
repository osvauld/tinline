//! Owner consent to cooperatively delete a specific cached profile picture (an `Issued`
//! statement for anyone).
//! This is not a capability to mutate a profile, delete chat media, or erase another owner's data.
use crate::{Error, IssuedClaim, Issued, Result, Signed, Subject};
use identity::Identity;
use serde::{Deserialize, Serialize};
pub const PICTURE_DELETE_SCOPE: &str = "cached-profile-picture";

/// The claim: the owner consents to cached copies of this picture being deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictureDeletion {
    pub picture_hash: String,
    pub scope: String,
}

/// Consent is not time-bound; the window only keeps the shape of every issued statement.
pub const PICTURE_DELETION_TTL_MS: u64 = 10 * 365 * 24 * 3600 * 1000;

impl IssuedClaim for PictureDeletion {
    const DOMAIN: cryptography::domains::Domain = cryptography::domains::PICTURE_DELETION;
    const MAX_TTL_MS: u64 = PICTURE_DELETION_TTL_MS;
    const MAX_BODY: usize = 2048;
    fn check(&self) -> Result<()> {
        if self.scope != PICTURE_DELETE_SCOPE || !valid_hash(&self.picture_hash) {
            return Err(Error::Decode);
        }
        Ok(())
    }
}

/// Issued by the picture's owner to anyone holding a cached copy.
pub type PictureDeletionPermit = Issued<PictureDeletion>;
pub type SignedPictureDeletion = Signed<PictureDeletionPermit>;

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The caller has obtained the owner's explicit consent. Receivers only delete this owner's
/// cached copy of this hash, never the current picture or another owner's/chat references.
pub fn sign_picture_deletion(id: &Identity, picture_hash: &str, now_ms: u64) -> Result<SignedPictureDeletion> {
    let claim = PictureDeletion { picture_hash: picture_hash.into(), scope: PICTURE_DELETE_SCOPE.into() };
    crate::issued::issue(id, Subject::Anyone, now_ms, PICTURE_DELETION_TTL_MS, claim)
}

/// A permit as kept in the owner's profile doc: signed by `expected_owner`, for anyone. A stored
/// permit stays valid (consent is not time-bound), so no clock is involved.
pub fn verify_picture_deletion(
    blob: &SignedPictureDeletion,
    expected_owner: &str,
) -> Result<PictureDeletionPermit> {
    let p = blob.open_from(expected_owner)?;
    if p.sub != Subject::Anyone {
        return Err(Error::WrongHolder);
    }
    Ok(p)
}

#[cfg(test)]
mod tests;
