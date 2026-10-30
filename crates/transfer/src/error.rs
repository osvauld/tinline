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
    /// The peer sent bytes that do not match the hash we asked for (bao verification failed at
    /// a chunk). The verified part before it is kept; this peer's copy is bad or tampered.
    #[error("data does not match the signed hash: {0}")]
    Corrupt(String),
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
