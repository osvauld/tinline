//! Waiting on a `Peer`'s call events.

use std::time::{Duration, Instant};

use p2pcore::CallState;

use super::{Ev, Peer};

/// Next event matching `f`, failing the test after `secs`.
pub fn expect<T>(p: &Peer, secs: u64, what: &str, mut f: impl FnMut(&Ev) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match p.rx.recv_timeout(left) {
            Ok(ev) => {
                if let Some(t) = f(&ev) {
                    return t;
                }
            }
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

/// Matches the end of call `id`, yielding its reason.
pub fn ended(id: &str) -> impl FnMut(&Ev) -> Option<String> + '_ {
    move |ev| match ev {
        Ev::State(i, CallState::Ended { reason }) if i == id => Some(reason.clone()),
        _ => None,
    }
}

/// The id of the next incoming call.
pub fn incoming(p: &Peer, secs: u64) -> String {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p
            .rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(Ev::Incoming(c)) => return c.call_id,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for an incoming call"),
        }
    }
}

/// Reason call `id` ended and how long after `t0`.
pub fn ended_after(p: &Peer, id: &str, t0: Instant, secs: u64) -> (String, Duration) {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p
            .rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(Ev::State(i, CallState::Ended { reason })) if i == id => {
                return (reason, t0.elapsed());
            }
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for call {id} to end"),
        }
    }
}

/// Whether an incoming call is already queued.
pub fn saw_incoming(p: &Peer) -> bool {
    p.rx.try_iter().any(|e| matches!(e, Ev::Incoming(_)))
}
