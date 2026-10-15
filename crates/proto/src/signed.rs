//! The one signed envelope. Every statement someone signs is a `Signable` struct with its own
//! domain, size cap and signer; `Signed<T>` is its postcard body plus an Ed25519 signature over
//! `sign::input(T::DOMAIN, [body])`. Opening decodes canonically (one byte string per value, no
//! trailing bytes, bounded), runs the type's shape check, and verifies strictly against the
//! signer the body itself names. Callers then compare that signer with who they expected
//! (`open_from`), and keep every policy check (expiry, membership, cut-offs) outside.
//!
//! Wire form: `to_bytes()` is `body || sig[64]`. In JSON frames a `Signed<T>` is one base64url
//! string of those bytes; in binary (postcard) rows it is a byte string.

use std::fmt;
use std::marker::PhantomData;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cryptography::{codec, domains::Domain, hash, sign, signature};
use identity::{Identity, did_from_public_key, public_key_from_did};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};

use crate::{Error, Result};

pub const SIG_LEN: usize = 64;

/// Who must have signed a statement, as the statement names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signer<'a> {
    /// An account DID (`did:key`); its public key is in the DID.
    Did(&'a str),
    /// A raw Ed25519 public key (a ticket's carried account key, a device key).
    Key(&'a [u8; 32]),
}

/// An owned [`Signer`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SignerId {
    Did(String),
    Key([u8; 32]),
}

impl Signer<'_> {
    pub fn public_key(&self) -> Result<[u8; 32]> {
        match self {
            Signer::Did(did) => public_key_from_did(did).ok_or(Error::Decode),
            Signer::Key(k) => Ok(**k),
        }
    }

    /// Whether this signer is the account `did` (a raw key counts if it is that DID's key).
    pub fn is_did(&self, did: &str) -> bool {
        match self {
            Signer::Did(d) => *d == did,
            Signer::Key(k) => did_from_public_key(k) == did,
        }
    }

    pub fn to_owned(&self) -> SignerId {
        match self {
            Signer::Did(d) => SignerId::Did(d.to_string()),
            Signer::Key(k) => SignerId::Key(**k),
        }
    }
}

/// A statement that travels signed.
pub trait Signable: Serialize + DeserializeOwned {
    /// Domain separation; one per type (`cryptography::domains`, checked unique).
    const DOMAIN: Domain;
    /// Largest encoded body accepted or produced.
    const MAX_BODY: usize;
    /// Who must have signed it.
    fn signer(&self) -> Signer<'_>;
    /// Shape and bounds only, never policy. Runs on sign and on open.
    fn check(&self) -> Result<()> {
        Ok(())
    }
}

/// Something that can sign: the account identity, or a raw secret key.
pub trait SignKey {
    fn public_key(&self) -> [u8; 32];
    fn sign_message(&self, message: &[u8]) -> [u8; 64];
}

impl SignKey for Identity {
    fn public_key(&self) -> [u8; 32] {
        self.signing_public_key()
    }
    fn sign_message(&self, message: &[u8]) -> [u8; 64] {
        self.sign(message)
    }
}

/// A raw Ed25519 secret key (the per-install device key).
#[derive(Clone, Copy)]
pub struct SecretKey<'a>(pub &'a [u8; 32]);

impl SignKey for SecretKey<'_> {
    fn public_key(&self) -> [u8; 32] {
        signature::public_key(self.0)
    }
    fn sign_message(&self, message: &[u8]) -> [u8; 64] {
        signature::sign(self.0, message)
    }
}

impl<K: SignKey + ?Sized> SignKey for &K {
    fn public_key(&self) -> [u8; 32] {
        (**self).public_key()
    }
    fn sign_message(&self, message: &[u8]) -> [u8; 64] {
        (**self).sign_message(message)
    }
}

/// A signed `T`: its canonical body and the signature over it.
pub struct Signed<T> {
    body: Vec<u8>,
    sig: [u8; SIG_LEN],
    _t: PhantomData<fn() -> T>,
}

impl<T: Signable> Signed<T> {
    /// Signs `claim` with `key`, which must be the key `claim.signer()` names.
    pub fn sign(claim: &T, key: &impl SignKey) -> Result<Self> {
        claim.check()?;
        if claim.signer().public_key()? != key.public_key() {
            return Err(Error::WrongIssuer);
        }
        let body = codec::to_bytes(claim).map_err(|_| Error::Decode)?;
        if body.len() > T::MAX_BODY {
            return Err(Error::TooLarge);
        }
        let sig = key.sign_message(&sign::input(T::DOMAIN, &[&body]));
        Ok(Self { body, sig, _t: PhantomData })
    }

    /// Decodes, checks shape and verifies against the signer the body names.
    pub fn open(&self) -> Result<T> {
        let claim: T = codec::from_bytes_canonical(&self.body, T::MAX_BODY).map_err(|e| match e {
            codec::CodecError::TooLarge => Error::TooLarge,
            _ => Error::Decode,
        })?;
        claim.check()?;
        let public = claim.signer().public_key()?;
        if !signature::verify(&public, &sign::input(T::DOMAIN, &[&self.body]), &self.sig) {
            return Err(Error::BadSignature);
        }
        Ok(claim)
    }

    /// [`open`](Self::open), and the signer is the account `did`.
    pub fn open_from(&self, did: &str) -> Result<T> {
        let claim = self.open()?;
        if !claim.signer().is_did(did) {
            return Err(Error::WrongIssuer);
        }
        Ok(claim)
    }

    /// The body WITHOUT verifying anything: bookkeeping only (which slot, whose list), never
    /// admission.
    pub fn peek(&self) -> Result<T> {
        codec::from_bytes(&self.body, T::MAX_BODY).map_err(|_| Error::Decode)
    }

    /// Who the body claims signed it, unverified.
    pub fn peek_signer(&self) -> Result<SignerId> {
        Ok(self.peek()?.signer().to_owned())
    }

    /// `body || sig`.
    pub fn to_bytes(&self) -> Vec<u8> {
        [&self.body[..], &self.sig[..]].concat()
    }

    /// Splits `body || sig`; bounded. Nothing is verified until `open`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > T::MAX_BODY + SIG_LEN {
            return Err(Error::TooLarge);
        }
        let split = bytes.len().checked_sub(SIG_LEN).ok_or(Error::Decode)?;
        Ok(Self {
            body: bytes[..split].to_vec(),
            sig: bytes[split..].try_into().expect("64 bytes"),
            _t: PhantomData,
        })
    }

    /// Content id: `hash::content(T::DOMAIN, to_bytes())`.
    pub fn id(&self) -> [u8; 32] {
        hash::content(T::DOMAIN, &self.to_bytes())
    }

    /// Length of `to_bytes()`.
    pub fn len(&self) -> usize {
        self.body.len() + SIG_LEN
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn sig(&self) -> &[u8; SIG_LEN] {
        &self.sig
    }

    /// Reassembles an envelope from a body and a signature carried apart (e.g. a roster entry).
    pub fn from_parts(body: Vec<u8>, sig: [u8; SIG_LEN]) -> Self {
        Self { body, sig, _t: PhantomData }
    }
}

/// A signature over `parts` under `domain` with no body travelling: the verifier rebuilds the
/// parts from what it already knows.
pub fn sign_detached(key: &impl SignKey, domain: Domain, parts: &[&[u8]]) -> [u8; 64] {
    key.sign_message(&sign::input(domain, parts))
}

/// Checks a [`sign_detached`] signature against `signer`.
pub fn verify_detached(signer: Signer<'_>, domain: Domain, parts: &[&[u8]], sig: &[u8; 64]) -> Result<()> {
    let public = signer.public_key()?;
    if signature::verify(&public, &sign::input(domain, parts), sig) {
        Ok(())
    } else {
        Err(Error::BadSignature)
    }
}

impl<T> Clone for Signed<T> {
    fn clone(&self) -> Self {
        Self { body: self.body.clone(), sig: self.sig, _t: PhantomData }
    }
}

impl<T> PartialEq for Signed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.body == other.body && self.sig == other.sig
    }
}

impl<T> Eq for Signed<T> {}

impl<T> fmt::Debug for Signed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signed({} body bytes)", self.body.len())
    }
}

impl<T: Signable> Serialize for Signed<T> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let bytes = self.to_bytes();
        if s.is_human_readable() {
            s.serialize_str(&URL_SAFE_NO_PAD.encode(bytes))
        } else {
            s.serialize_bytes(&bytes)
        }
    }
}

impl<'de, T: Signable> Deserialize<'de> for Signed<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        use serde::de::Error as _;
        let bytes = if d.is_human_readable() {
            let text = String::deserialize(d)?;
            URL_SAFE_NO_PAD.decode(text).map_err(D::Error::custom)?
        } else {
            serde_bytes_vec(d)?
        };
        Self::from_bytes(&bytes).map_err(D::Error::custom)
    }
}

fn serde_bytes_vec<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<u8>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Vec<u8>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("signed bytes")
        }
        fn visit_bytes<E>(self, v: &[u8]) -> std::result::Result<Vec<u8>, E> {
            Ok(v.to_vec())
        }
        fn visit_byte_buf<E>(self, v: Vec<u8>) -> std::result::Result<Vec<u8>, E> {
            Ok(v)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Vec<u8>, A::Error> {
            let mut out = Vec::new();
            while let Some(b) = seq.next_element()? {
                out.push(b);
            }
            Ok(out)
        }
    }
    d.deserialize_bytes(V)
}

/// The per-type property table (forgery, cross-domain replay, trailing bytes, wrong signer)
/// every `Signable` runs in its tests.
#[cfg(any(test, feature = "test-support"))]
pub mod conformance;

#[cfg(test)]
mod tests;
