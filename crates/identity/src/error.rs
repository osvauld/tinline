use cryptography::CryptoError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("invalid mnemonic")]
    Bip39,
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
}
