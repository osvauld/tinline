//! Owner consent to cooperatively delete a specific cached profile picture.
//! This is not a capability to mutate a profile, delete chat media, or erase another owner's data.
use crate::{Error, Result, Signable, Signed, Signer};
use identity::Identity;
use serde::{Deserialize, Serialize};
pub const PICTURE_DELETE_SCOPE: &str = "cached-profile-picture";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictureDeletionPermit {
    pub owner: String,
    pub picture_hash: String,
    pub scope: String,
    pub consent: bool,
}

impl Signable for PictureDeletionPermit {
    const DOMAIN: cryptography::domains::Domain = cryptography::domains::PICTURE_DELETION;
    const MAX_BODY: usize = 2048;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.owner)
    }
    fn check(&self) -> Result<()> {
        if !self.consent || self.scope != PICTURE_DELETE_SCOPE || !valid_hash(&self.picture_hash) {
            return Err(Error::Decode);
        }
        Ok(())
    }
}

pub type SignedPictureDeletion = Signed<PictureDeletionPermit>;

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
/// The caller has obtained the owner's explicit consent. Receivers only delete this owner's
/// cached copy of this hash, never the current picture or another owner's/chat references.
pub fn sign_picture_deletion(id: &Identity, picture_hash: &str) -> Result<SignedPictureDeletion> {
    Signed::sign(
        &PictureDeletionPermit {
            owner: id.did().into(),
            picture_hash: picture_hash.into(),
            scope: PICTURE_DELETE_SCOPE.into(),
            consent: true,
        },
        id,
    )
}
pub fn verify_picture_deletion(
    blob: &SignedPictureDeletion,
    expected_owner: &str,
) -> Result<PictureDeletionPermit> {
    blob.open_from(expected_owner)
}

#[cfg(test)]
mod tests;
