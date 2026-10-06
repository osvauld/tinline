// DID = "did:key:z" + base58btc( multicodec(ed25519-pub) || pubkey ).
// Self-authenticating: the signing public key is embedded, so no resolution is needed.

const MULTICODEC_ED25519: [u8; 2] = [0xed, 0x01];
const DID_PREFIX: &str = "did:key:z";

pub fn did_from_public_key(signing_public: &[u8; 32]) -> String {
    let mut bytes = Vec::with_capacity(2 + 32);
    bytes.extend_from_slice(&MULTICODEC_ED25519);
    bytes.extend_from_slice(signing_public);
    format!("{DID_PREFIX}{}", bs58::encode(bytes).into_string())
}

pub fn public_key_from_did(did: &str) -> Option<[u8; 32]> {
    let encoded = did.strip_prefix(DID_PREFIX)?;
    let bytes = bs58::decode(encoded).into_vec().ok()?;
    if bytes.len() != 34 || bytes[0..2] != MULTICODEC_ED25519 {
        return None;
    }
    bytes[2..].try_into().ok()
}

#[cfg(test)]
mod tests;
