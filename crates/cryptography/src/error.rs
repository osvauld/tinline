use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("aead operation failed")]
    Aead,
    #[error("ciphertext too short")]
    CiphertextTooShort,
    #[error("argon2: {0}")]
    Argon2(String),
}
