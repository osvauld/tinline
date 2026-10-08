#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum Error {
    #[error("no identity yet")]
    NoIdentity,
    #[error("an identity already exists")]
    HaveIdentity,
    #[error("that account is already on this device")]
    AccountExists,
    #[error("a call is in progress")]
    InCall,
    #[error("that recovery phrase is not valid")]
    BadPhrase,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("identity is locked")]
    Locked,
    #[error("the passphrase cannot be empty")]
    WeakPassphrase,
    #[error("node not started")]
    NotStarted,
    #[error("not found")]
    NotFound,
    #[error("already in a call")]
    Busy,
    #[error("timed out")]
    Timeout,
    #[error("they refused: {0}")]
    Rejected(String),
    #[error("{0}")]
    Protocol(String),
    #[error("network: {0}")]
    Net(String),
    #[error("storage: {0}")]
    Io(String),
    #[error("audio: {0}")]
    Audio(String),
}

impl Error {
    pub fn net(e: impl std::fmt::Display) -> Self {
        Error::Net(e.to_string())
    }
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

impl From<audio::Error> for Error {
    fn from(e: audio::Error) -> Self {
        Error::Audio(e.to_string())
    }
}
