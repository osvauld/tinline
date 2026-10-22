//! The one binary encoding for structs we store, send or sign: postcard 1 (varint lengths and
//! integers, positional fields). Decoding is bounded (`max` before anything is parsed) and strict
//! (trailing bytes are an error). Postcard accepts overlong varints, so a value has more than one
//! byte form; anything signed or hashed into an id decodes with [`from_bytes_canonical`], which
//! also requires the bytes to be exactly what [`to_bytes`] would write.

use serde::{Serialize, de::DeserializeOwned};

/// Most bytes a postcard length prefix (a `usize`/`u64` varint) takes; for size formulas.
pub const LEN_PREFIX_MAX: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    #[error("encoded value too large")]
    TooLarge,
    #[error("bytes do not decode")]
    Decode,
    #[error("bytes are not in canonical form")]
    NonCanonical,
    #[error("value does not encode")]
    Encode,
}

/// Encodes `v`. Fails only for types whose `Serialize` itself fails (none of ours).
pub fn to_bytes<T: Serialize + ?Sized>(v: &T) -> Result<Vec<u8>, CodecError> {
    postcard::to_allocvec(v).map_err(|_| CodecError::Encode)
}

/// [`to_bytes`] for our own derive-`Serialize` types, which cannot fail to encode. Should one
/// ever (a hand-written `Serialize` that errors), the result is empty rather than a panic, and
/// the decode of it fails closed.
pub fn to_bytes_infallible<T: Serialize + ?Sized>(v: &T) -> Vec<u8> {
    to_bytes(v).unwrap_or_default()
}

/// Decodes `bytes` (at most `max` long) as exactly one `T`, with nothing left over.
pub fn from_bytes<T: DeserializeOwned>(bytes: &[u8], max: usize) -> Result<T, CodecError> {
    if bytes.len() > max {
        return Err(CodecError::TooLarge);
    }
    let (v, rest) = postcard::take_from_bytes(bytes).map_err(|_| CodecError::Decode)?;
    if !rest.is_empty() {
        return Err(CodecError::Decode);
    }
    Ok(v)
}

/// [`from_bytes`], and the bytes must re-encode to themselves: one byte string per value, so a
/// signature or id over them cannot be replayed under a second spelling.
pub fn from_bytes_canonical<T: DeserializeOwned + Serialize>(bytes: &[u8], max: usize) -> Result<T, CodecError> {
    let v: T = from_bytes(bytes, max)?;
    if to_bytes(&v)? != bytes {
        return Err(CodecError::NonCanonical);
    }
    Ok(v)
}

#[cfg(test)]
mod tests;
