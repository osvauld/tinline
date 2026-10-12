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
mod tests {
    use super::*;
    #[test]
    fn permit_binds_owner_hash_consent_and_scope_and_rejects_other_domains() {
        let (a, _) = identity::generate();
        let (b, _) = identity::generate();
        let hash = "a".repeat(64);
        let blob = sign_picture_deletion(&a, &hash).unwrap();
        assert_eq!(
            verify_picture_deletion(&blob, a.did())
                .unwrap()
                .picture_hash,
            hash
        );
        assert!(verify_picture_deletion(&blob, b.did()).is_err());
        let mut p = verify_picture_deletion(&blob, a.did()).unwrap();
        p.picture_hash = "b".repeat(64);
        let tampered = SignedBlob {
            payload: crate::enc(serde_json::to_vec(&p).unwrap()),
            ..blob.clone()
        };
        assert!(verify_picture_deletion(&tampered, a.did()).is_err());
        p.consent = false;
        assert!(verify_picture_deletion(&sign_blob(&a, DOMAIN, &p), a.did()).is_err());
        p.consent = true;
        p.scope = "delete-chat-history".into();
        assert!(verify_picture_deletion(&sign_blob(&a, DOMAIN, &p), a.did()).is_err());
        assert!(
            verify_picture_deletion(&crate::sign_block_notice(&a, b.did(), true, 1), a.did())
                .is_err()
        );
        assert!(sign_picture_deletion(&a, "not-a-hash").is_err());
    }
}
