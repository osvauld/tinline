use cryptography::CryptoError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("invalid mnemonic")]
    Bip39,
    #[error("corrupt or unsupported keystore")]
    Decode,
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
}
