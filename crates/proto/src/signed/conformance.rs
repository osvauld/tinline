//! One table of envelope properties, run for every `Signable` with a sample value.

use std::fmt::Debug;

use serde::{Deserialize, Serialize};

use super::*;

/// The same body under another domain: what a cross-type replay looks like.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct Twin<T>(T);

impl<T: Signable> Signable for Twin<T> {
    const DOMAIN: Domain = b"test/conformance/twin";
    const MAX_BODY: usize = T::MAX_BODY;
    fn signer(&self) -> Signer<'_> {
        self.0.signer()
    }
}

/// Runs the table for `sample`, signed by `key` (its signer); `stranger` is any other key.
pub fn check<T: Signable + PartialEq + Debug>(sample: &T, key: &impl SignKey, stranger: &impl SignKey) {
    let name = std::any::type_name::<T>();
    let signed = Signed::sign(sample, key).unwrap_or_else(|e| panic!("{name}: sign: {e}"));

    // Round trip, through bytes too.
    assert_eq!(&signed.open().unwrap(), sample, "{name}: open");
    let back = Signed::<T>::from_bytes(&signed.to_bytes()).unwrap();
    assert_eq!(back, signed, "{name}: bytes");
    assert_eq!(&back.open().unwrap(), sample, "{name}: open after bytes");
    if let Signer::Did(did) = sample.signer() {
        assert_eq!(&signed.open_from(did).unwrap(), sample, "{name}: open_from");
        let other = identity::did_from_public_key(&stranger.public_key());
        assert_eq!(signed.open_from(&other), Err(Error::WrongIssuer), "{name}: open_from other");
    }

    // Forgery: any flipped byte, body or signature, fails.
    let bytes = signed.to_bytes();
    for i in 0..bytes.len() {
        let mut b = bytes.clone();
        b[i] ^= 0x01;
        let opened = Signed::<T>::from_bytes(&b).and_then(|s| s.open());
        assert!(opened.is_err(), "{name}: flipped byte {i} opened");
    }

    // Cross-domain replay: the same bytes read as another statement type fail.
    let twin = Signed::<Twin<T>>::from_bytes(&bytes).unwrap();
    assert_eq!(twin.open().err(), Some(Error::BadSignature), "{name}: cross-domain");

    // Trailing bytes after the body, even correctly signed, are refused.
    let mut body = signed.body().to_vec();
    body.push(0);
    let sig = key.sign_message(&sign::input(T::DOMAIN, &[&body]));
    assert!(Signed::<T>::from_parts(body, sig).open().is_err(), "{name}: trailing byte");

    // A signer that is not the one the body names is refused at sign time, and a signature by
    // it does not open.
    assert_eq!(Signed::sign(sample, stranger).err(), Some(Error::WrongIssuer), "{name}: wrong signer");
    let body = signed.body().to_vec();
    let sig = stranger.sign_message(&sign::input(T::DOMAIN, &[&body]));
    assert_eq!(Signed::<T>::from_parts(body, sig).open().err(), Some(Error::BadSignature), "{name}: stranger sig");

    // The size cap holds on bytes.
    let huge = vec![0u8; T::MAX_BODY + SIG_LEN + 1];
    assert_eq!(Signed::<T>::from_bytes(&huge).err(), Some(Error::TooLarge), "{name}: cap");
}
