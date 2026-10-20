/// Why an index operation failed. p2pcore maps it into its own `Error` (same variants as before
/// the split).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The store could not be opened, read or written; carries the store's message.
    #[error("storage: {0}")]
    Io(String),
    /// A bad request: no term, or a cursor this index did not hand out.
    #[error("{0}")]
    Protocol(String),
}

impl From<storage::StorageError> for Error {
    fn from(e: storage::StorageError) -> Self {
        Error::Io(e.to_string())
    }
}
