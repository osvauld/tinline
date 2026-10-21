/// Why an account model operation failed. p2pcore maps it into its own `Error` (same variants
/// and messages as before the split).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The phrase belongs to an account already on this device (carries its DID).
    #[error("that account is already on this device ({0})")]
    AccountExists(String),
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("the passphrase cannot be empty")]
    WeakPassphrase,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Protocol(String),
    #[error("storage: {0}")]
    Io(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<proto::Error> for Error {
    fn from(e: proto::Error) -> Self {
        Error::Protocol(e.to_string())
    }
}

impl From<crdt::Error> for Error {
    fn from(e: crdt::Error) -> Self {
        match e {
            crdt::Error::Io(s) => Error::Io(s),
            crdt::Error::MissingDeps => Error::Protocol(e.to_string()),
        }
    }
}
