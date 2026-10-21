/// Why a transfer operation failed. p2pcore maps it into its own `Error` (same variants and
/// messages as before the split).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A fetch made no progress for the stall window.
    #[error("timed out")]
    Timeout,
    /// The blob does not match what its descriptor promised.
    #[error("{0}")]
    Protocol(String),
    #[error("network: {0}")]
    Net(String),
    #[error("storage: {0}")]
    Io(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}
