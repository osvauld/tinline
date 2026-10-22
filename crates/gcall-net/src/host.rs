//! The seam to the node: what the service needs from it, and its error type.

use identity::Identity;
use iroh::Endpoint;

/// What the group-call service needs from the node that owns it. p2pcore implements it for its
/// `Inner`; the service holds a `Weak` to it (see the crate docs for the lock order).
pub trait GcallHost: Send + Sync + 'static {
    /// This device's identity (`Locked` / `NoIdentity` while there is none).
    fn identity(&self) -> Result<Identity, Error>;
    /// The group endpoint, if it is up. Looked up at each dial: it may rotate.
    fn group_endpoint(&self) -> Option<Endpoint>;
    /// Runs `f` while no 1:1 call is active and none can begin, else `Err(InCall)`. The node
    /// holds its `live` lock around `f` (lock order: live -> gcall session).
    fn with_no_call(&self, f: &mut dyn FnMut() -> Result<(), Error>) -> Result<(), Error>;
}

/// Errors of the service. Messages are those of the matching p2pcore `Error` variants, which
/// they map to one to one.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no identity yet")]
    NoIdentity,
    #[error("a call is in progress")]
    InCall,
    #[error("identity is locked")]
    Locked,
    #[error("node not started")]
    NotStarted,
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
}

impl Error {
    pub fn net(e: impl std::fmt::Display) -> Self {
        Error::Net(e.to_string())
    }
}
