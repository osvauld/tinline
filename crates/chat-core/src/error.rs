/// Why a chat store operation failed. p2pcore maps it into its own `Error` (same variants and
/// messages as before the split).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The vault could not be read or written, or a row is malformed; carries the reason.
    #[error("storage: {0}")]
    Io(String),
}

impl From<storage::StorageError> for Error {
    fn from(e: storage::StorageError) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Io(e.to_string())
    }
}
