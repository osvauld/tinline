//! Owner consent to cooperatively delete a specific cached profile picture.
//! This is not a capability to mutate a profile, delete chat media, or erase another owner's data.
use crate::{Error, Result, SignedBlob, VERSION, check_version, open_blob, sign_blob};
use identity::Identity;
use serde::{Deserialize, Serialize};
const DOMAIN: &[u8] = b"tinline-profile-picture-deletion-v1\0";
pub const PICTURE_DELETE_SCOPE: &str = "cached-profile-picture";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PictureDeletionPermit {
    pub v: u8,
    pub owner: String,
    pub picture_hash: String,
    pub scope: String,
    pub consent: bool,
}
fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
/// The caller has obtained the owner's explicit consent. Receivers only delete this owner's
/// cached copy of this hash, never the current picture or another owner's/chat references.
pub fn sign_picture_deletion(id: &Identity, picture_hash: &str) -> Result<SignedBlob> {
    if !valid_hash(picture_hash) {
        return Err(Error::Decode);
    }
    Ok(sign_blob(
        id,
        DOMAIN,
        &PictureDeletionPermit {
            v: VERSION,
            owner: id.did().into(),
            picture_hash: picture_hash.into(),
            scope: PICTURE_DELETE_SCOPE.into(),
            consent: true,
        },
    ))
}
pub fn verify_picture_deletion(
    blob: &SignedBlob,
    expected_owner: &str,
) -> Result<PictureDeletionPermit> {
    if blob.payload.len() > 2048 || blob.signature.len() > 128 {
        return Err(Error::Decode);
    }
    let p: PictureDeletionPermit = open_blob(blob, DOMAIN, |p: &PictureDeletionPermit| &p.owner)?;
    check_version(p.v)?;
    if p.owner != expected_owner {
        return Err(Error::WrongIssuer);
    }
    if !p.consent || p.scope != PICTURE_DELETE_SCOPE || !valid_hash(&p.picture_hash) {
        return Err(Error::Decode);
    }
    Ok(p)
}

#[cfg(test)]
mod tests;
