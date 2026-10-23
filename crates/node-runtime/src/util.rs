//! Small helpers: clocks, ids, thread detection, timeouts.

use super::*;

pub fn waiting_ring_timeout() -> Duration {
    std::env::var("P2P_WAITING_RING_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(WAITING_RING_TIMEOUT, Duration::from_secs)
}

/// How long `stop`, `lock` and an account switch wait for the endpoint to close.
/// `P2P_CLOSE_WAIT_MS` overrides it, for tests.
pub fn close_wait() -> Duration {
    std::env::var("P2P_CLOSE_WAIT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(CLOSE_WAIT, Duration::from_millis)
}

/// A reason string from the peer, for the UI: no control characters, bounded length.
pub fn clip_reason(reason: &str) -> String {
    reason.chars().filter(|c| !c.is_control()).take(MAX_REASON_CHARS).collect()
}

pub fn msg_name(m: &Msg) -> &'static str {
    match m {
        Msg::ContactHello { .. } => "ContactHello",
        Msg::ContactWelcome { .. } => "ContactWelcome",
        Msg::Reject { .. } => "Reject",
        Msg::CallHello { .. } => "CallHello",
        Msg::Ringing => "Ringing",
        Msg::Accept { .. } => "Accept",
        Msg::Cancel { .. } => "Cancel",
        Msg::Decline { .. } => "Decline",
        Msg::Busy => "Busy",
        Msg::Hangup => "Hangup",
        Msg::Notice { .. } => "Notice",
        Msg::NoticeTaken => "NoticeTaken",
        Msg::Tokens { .. } => "Tokens",
        Msg::TokensTaken => "TokensTaken",
        Msg::Devices { .. } => "Devices",
        Msg::DevicesTaken => "DevicesTaken",
        Msg::GroupLetter { .. } => "GroupLetter",
        Msg::GroupLetterTaken => "GroupLetterTaken",
        Msg::ContactPending => "ContactPending",
        Msg::WelcomeTaken => "WelcomeTaken",
        Msg::Video { .. } => "Video",
        Msg::VideoCodec { .. } => "VideoCodec",
    }
}

/// The error for a reply that is not the expected "taken": `<what> not taken: <message kind>`
/// (`Net`, "no reply", when the stream closed instead), never the message's contents.
pub fn not_taken(what: &str, reply: Option<&Msg>) -> Error {
    match reply {
        Some(m) => Error::Protocol(format!("{what} not taken: {}", msg_name(m))),
        None => Error::eof(&format!("{what} not taken: no reply")),
    }
}

/// Every thread of the core runtime carries this name (workers and blocking pool alike).
pub const CORE_THREAD: &str = "p2pcore";

pub fn on_core_thread() -> bool {
    std::thread::current().name() == Some(CORE_THREAD)
}

tokio::task_local! {
    /// Set around the future of a blocking `Node` call (`block_on`).
    pub static USER_CALL: ();
}

/// Whether we run for a call the app made (its own thread, or the future of a blocking call),
/// not for a background task of the core.
pub fn user_call() -> bool {
    !on_core_thread() || USER_CALL.try_with(|_| ()).is_ok()
}

// The wall clock lives in `crdt` so the receipt ledger shares it.
pub use crdt::now_ms;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn random_id() -> String {
    let mut b = [0u8; 8];
    cryptography::rng::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Whether `new` is a good grant from `issuer` to `holder` and expires after `old` (any
/// capability; the token slots use `caps::outlives`).
#[cfg(test)]
pub fn grant_outlives(old: &proto::SignedGrant, new: &proto::SignedGrant, issuer: &str, holder: &str, now_ms: u64) -> bool {
    let e = proto::Expect { iss: Some(issuer), holder, now_ms };
    let Ok(new) = new.redeem(&e, &proto::Unrecorded) else { return false };
    // An old grant that no longer verifies (expired) is beaten by any valid new one.
    let old_exp = old.redeem(&e, &proto::Unrecorded).map_or(0, |g| g.exp_ms);
    new.exp_ms > old_exp
}

/// Turns a failed read or write into a warning instead of silence, for call sites that cannot
/// return the error. The result is the value, or `None` / the default when it failed.
pub trait OrLog<T> {
    /// `Some(value)` on success (`None` for a missing value), `None` after logging on `Err`.
    fn or_log(self, what: &str) -> Option<T>;
    /// The value, or `T::default()` after logging on `Err`.
    fn or_log_default(self, what: &str) -> T
    where
        T: Default;
}

impl<T, E: std::fmt::Display> OrLog<T> for Result<T, E> {
    fn or_log(self, what: &str) -> Option<T> {
        self.map_err(|e| tracing::warn!("{what}: {e}")).ok()
    }

    fn or_log_default(self, what: &str) -> T
    where
        T: Default,
    {
        self.or_log(what).unwrap_or_default()
    }
}

/// `Result<Option<T>, E>` flattened: an absent value and a failed read both give `None`, and
/// only the failure is logged.
pub trait OrLogFlat<T> {
    fn or_log_flat(self, what: &str) -> Option<T>;
}

impl<T, E: std::fmt::Display> OrLogFlat<T> for Result<Option<T>, E> {
    fn or_log_flat(self, what: &str) -> Option<T> {
        self.or_log(what).flatten()
    }
}
