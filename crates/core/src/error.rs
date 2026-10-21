#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum Error {
    #[error("no identity yet")]
    NoIdentity,
    #[error("an identity already exists")]
    HaveIdentity,
    /// The phrase belongs to the account that is open right now (carries its DID): switch to or
    /// unlock it instead of restoring.
    #[error("that account is already on this device ({0})")]
    AccountExists(String),
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

impl From<search_core::Error> for Error {
    fn from(e: search_core::Error) -> Self {
        match e {
            search_core::Error::Io(s) => Error::Io(s),
            search_core::Error::Protocol(s) => Error::Protocol(s),
        }
    }
}

impl From<group_core::Error> for Error {
    fn from(e: group_core::Error) -> Self {
        match e {
            group_core::Error::Io(s) => Error::Io(s),
            group_core::Error::Protocol(s) => Error::Protocol(s),
            group_core::Error::Rejected(s) => Error::Rejected(s),
        }
    }
}

impl From<account_model::Error> for Error {
    fn from(e: account_model::Error) -> Self {
        match e {
            account_model::Error::AccountExists(s) => Error::AccountExists(s),
            account_model::Error::WrongPassphrase => Error::WrongPassphrase,
            account_model::Error::WeakPassphrase => Error::WeakPassphrase,
            account_model::Error::NotFound => Error::NotFound,
            account_model::Error::Protocol(s) => Error::Protocol(s),
            account_model::Error::Io(s) => Error::Io(s),
        }
    }
}

impl From<transfer::Error> for Error {
    fn from(e: transfer::Error) -> Self {
        match e {
            transfer::Error::Timeout => Error::Timeout,
            transfer::Error::Protocol(s) => Error::Protocol(s),
            transfer::Error::Net(s) => Error::Net(s),
            transfer::Error::Io(s) => Error::Io(s),
        }
    }
}

impl From<audio::Error> for Error {
    fn from(e: audio::Error) -> Self {
        Error::Audio(e.to_string())
    }
}
