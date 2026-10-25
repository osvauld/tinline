//! base64url without padding, for tokens, hints, JWTs and the byte fields of the JSON API.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text).ok()
}

/// `#[serde(with = "crate::b64::array")]` for fixed-size byte fields in JSON.
pub mod array {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer, const N: usize>(
        bytes: &[u8; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        d: D,
    ) -> Result<[u8; N], D::Error> {
        let text = <&str>::deserialize(d)?;
        super::decode(text)
            .and_then(|v| <[u8; N]>::try_from(v).ok())
            .ok_or_else(|| D::Error::custom("bad base64url field"))
    }
}
