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

    /// A spawned task that panicked or was cancelled: our fault (`Io`), as in node-runtime.
    pub fn join(e: tokio::task::JoinError) -> Self {
        Error::Io(format!("task failed: {e}"))
    }
}

/// A `Reject` while reading our own shards is a storage fault (`Io`), as in node-runtime.
impl From<chat_core::doc::Reject> for Error {
    fn from(e: chat_core::doc::Reject) -> Self {
        Error::Io(e.to_string())
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

impl From<chat_core::Error> for Error {
    fn from(e: chat_core::Error) -> Self {
        match e {
            chat_core::Error::Io(s) => Error::Io(s),
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
            // A bad or tampered copy at one peer: another source may still be good.
            transfer::Error::Corrupt(s) => Error::Net(format!("data does not match the signed hash: {s}")),
            transfer::Error::Io(s) => Error::Io(s),
        }
    }
}

impl From<call_video::Error> for Error {
    fn from(e: call_video::Error) -> Self {
        match e {
            call_video::Error::NotFound => Error::NotFound,
            call_video::Error::Protocol(s) => Error::Protocol(s),
        }
    }
}

impl From<audio::Error> for Error {
    fn from(e: audio::Error) -> Self {
        Error::Audio(e.to_string())
    }
}

impl From<gcall_net::Error> for Error {
    fn from(e: gcall_net::Error) -> Self {
        use gcall_net::Error as G;
        match e {
            G::NoIdentity => Error::NoIdentity,
            G::InCall => Error::InCall,
            G::Locked => Error::Locked,
            G::NotStarted => Error::NotStarted,
            G::Busy => Error::Busy,
            G::Timeout => Error::Timeout,
            G::Rejected(s) => Error::Rejected(s),
            G::Protocol(s) => Error::Protocol(s),
            G::Net(s) => Error::Net(s),
        }
    }
}

impl From<Error> for gcall_net::Error {
    /// Only the variants the host closure can produce are distinguished; the rest are text.
    fn from(e: Error) -> Self {
        use gcall_net::Error as G;
        match e {
            Error::NoIdentity => G::NoIdentity,
            Error::InCall => G::InCall,
            Error::Locked => G::Locked,
            Error::NotStarted => G::NotStarted,
            Error::Busy => G::Busy,
            Error::Timeout => G::Timeout,
            Error::Rejected(s) => G::Rejected(s),
            Error::Protocol(s) => G::Protocol(s),
            Error::Net(s) => G::Net(s),
            other => G::Protocol(other.to_string()),
        }
    }
}

impl From<node_runtime::Error> for Error {
    fn from(e: node_runtime::Error) -> Self {
        use node_runtime::Error as N;
        match e {
            N::NoIdentity => Error::NoIdentity,
            N::HaveIdentity => Error::HaveIdentity,
            N::AccountExists(s) => Error::AccountExists(s),
            N::InCall => Error::InCall,
            N::BadPhrase => Error::BadPhrase,
            N::WrongPassphrase => Error::WrongPassphrase,
            N::Locked => Error::Locked,
            N::WeakPassphrase => Error::WeakPassphrase,
            N::NotStarted => Error::NotStarted,
            N::NotFound => Error::NotFound,
            N::Busy => Error::Busy,
            N::Timeout => Error::Timeout,
            N::Rejected(s) => Error::Rejected(s),
            N::Protocol(s) => Error::Protocol(s),
            N::Net(s) => Error::Net(s),
            N::Io(s) => Error::Io(s),
            N::Audio(s) => Error::Audio(s),
        }
    }
}

impl From<Error> for node_runtime::Error {
    fn from(e: Error) -> Self {
        use node_runtime::Error as N;
        match e {
            Error::NoIdentity => N::NoIdentity,
            Error::HaveIdentity => N::HaveIdentity,
            Error::AccountExists(s) => N::AccountExists(s),
            Error::InCall => N::InCall,
            Error::BadPhrase => N::BadPhrase,
            Error::WrongPassphrase => N::WrongPassphrase,
            Error::Locked => N::Locked,
            Error::WeakPassphrase => N::WeakPassphrase,
            Error::NotStarted => N::NotStarted,
            Error::NotFound => N::NotFound,
            Error::Busy => N::Busy,
            Error::Timeout => N::Timeout,
            Error::Rejected(s) => N::Rejected(s),
            Error::Protocol(s) => N::Protocol(s),
            Error::Net(s) => N::Net(s),
            Error::Io(s) => N::Io(s),
            Error::Audio(s) => N::Audio(s),
        }
    }
}
