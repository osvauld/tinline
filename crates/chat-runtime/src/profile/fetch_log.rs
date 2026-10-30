//! Rate limit for the one release-visible line a failing picture fetch logs (2026-10-29: a phone
//! showed initials for a contact's new picture for good, and nothing said why). The download is
//! retried on a backoff and re-kicked every 30 s, so a reason is logged when it first appears,
//! when it changes, and at most every [`REPEAT_EVERY`] while it stays the same.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The same failure of one picture is logged again after this long (PROVISIONAL).
pub const REPEAT_EVERY: Duration = Duration::from_secs(10 * 60);
/// Entries older than this are forgotten, so the map stays small.
const FORGET_AFTER: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct FetchLog {
    /// `(owner DID, picture hash)` -> the reason last logged and when.
    last: HashMap<(String, String), (String, Instant)>,
}

impl FetchLog {
    /// Whether `reason` for the picture `hash` of `did` should be logged at `now`; remembers it
    /// when so.
    pub fn should_log(&mut self, did: &str, hash: &str, reason: &str, now: Instant) -> bool {
        self.last
            .retain(|_, (_, at)| now.saturating_duration_since(*at) < FORGET_AFTER);
        let key = (did.to_string(), hash.to_string());
        let due = match self.last.get(&key) {
            Some((r, at)) => r != reason || now.saturating_duration_since(*at) >= REPEAT_EVERY,
            None => true,
        };
        if due {
            self.last.insert(key, (reason.to_string(), now));
        }
        due
    }

    /// The picture arrived: forget it. Whether a failure of it had been logged.
    pub fn succeeded(&mut self, did: &str, hash: &str) -> bool {
        self.last
            .remove(&(did.to_string(), hash.to_string()))
            .is_some()
    }
}

/// A fetch error as a short class for the log: never the text of a network error, which can
/// carry addresses.
pub fn class(e: &crate::Error) -> String {
    use crate::Error;
    match e {
        Error::Timeout => "stalled".into(),
        Error::Rejected(_) => "refused".into(),
        Error::Protocol(s) => s.chars().take(60).collect(),
        Error::Net(_) => "transfer failed".into(),
        Error::Io(_) => "storage error".into(),
        Error::Locked => "locked".into(),
        other => format!("{other}").chars().take(40).collect(),
    }
}

#[cfg(test)]
mod tests;
