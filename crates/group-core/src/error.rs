/// Why a group store or model operation failed. p2pcore maps it into its own `Error` (same
/// variants as before the split).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The vault could not be read or written, or a row is malformed; carries the reason.
    #[error("storage: {0}")]
    Io(String),
    /// Sealing or signing failed.
    #[error("{0}")]
    Protocol(String),
    /// A value the caller asked to store is not allowed (an invalid rotation policy).
    #[error("they refused: {0}")]
    Rejected(String),
}

impl From<storage::StorageError> for Error {
    fn from(e: storage::StorageError) -> Self {
        Error::Io(e.to_string())
    }
}
