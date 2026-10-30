//! The node runtime's error type. Variants and messages are those of p2pcore's `Error`, which
//! this maps to one to one (and back): an `InCall` stays an `InCall` across the crate boundary.
//!
//! Which variant, when it is not one of the plain state ones (`Locked`, `NotFound`, ...). Callers
//! decide on the variant, so keep to this:
//! - `Protocol`: the peer broke the rules, or the request is invalid. It will fail the same way
//!   again: do not retry (`images.rs` drops a host's image on it, chat downloads stop on it).
//! - `Net` / `Timeout`: transient. Connecting, a connection that closed or ended early (EOF,
//!   "peer closed", a stream that stopped mid-frame), an older app that refused the handshake.
//!   Retry later.
//! - `Io`: our own storage or local machinery failed (vault, rows we wrote, a background task
//!   that panicked or was cancelled: see `Error::join`).
//! - `Rejected`: the peer understood and said no (the reason is theirs to give).

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no identity yet")]
    NoIdentity,
    #[error("an identity already exists")]
    HaveIdentity,
    /// The phrase belongs to the account that is open right now (carries its DID).
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

    /// `what` is invalid or does not match, as seen from the caller: `Protocol("<what>: <why>")`.
    pub fn invalid(what: &str, why: impl std::fmt::Display) -> Self {
        Error::Protocol(format!("{what}: {why}"))
    }

    /// The connection or stream ended where more was due: transient (`Net`), not a rule broken.
    pub fn eof(what: &str) -> Self {
        Error::Net(what.to_string())
    }

    /// A `spawn_blocking` / spawned task that panicked or was cancelled: our fault (`Io`).
    pub fn join(e: tokio::task::JoinError) -> Self {
        Error::Io(format!("task failed: {e}"))
    }
}

/// A `chat_core::Reject` while reading or writing a shard in our own store is `Io` (corrupt or
/// unwritable rows). A `Reject` on a batch a peer sent is the peer's fault: map that one by hand
/// to `Protocol` (`chat_receive` does).
impl From<chat_core::doc::Reject> for Error {
    fn from(e: chat_core::doc::Reject) -> Self {
        Error::Io(e.to_string())
    }
}

/// Receipt-store failures come from our own store and bookkeeping (`Io`); a peer's bad receipt
/// is judged before it reaches the store and answered with `Protocol` there.
impl From<receipts_core::ReceiptError> for Error {
    fn from(e: receipts_core::ReceiptError) -> Self {
        Error::Io(format!("receipts: {e}"))
    }
}

/// Bytes that do not decode are a peer's (`Protocol`); a value of ours that does not encode is a
/// local fault (`Io`).
impl From<cryptography::codec::CodecError> for Error {
    fn from(e: cryptography::codec::CodecError) -> Self {
        use cryptography::codec::CodecError as C;
        match e {
            C::Encode => Error::Io(e.to_string()),
            C::TooLarge | C::Decode | C::NonCanonical => Error::Protocol(e.to_string()),
        }
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

impl From<group_core::Error> for Error {
    fn from(e: group_core::Error) -> Self {
        match e {
            group_core::Error::Io(s) => Error::Io(s),
            group_core::Error::Protocol(s) => Error::Protocol(s),
            group_core::Error::Rejected(s) => Error::Rejected(s),
        }
    }
}
