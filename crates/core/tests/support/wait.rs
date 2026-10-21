//! Polling and clock helpers.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use p2pcore::Node;

/// Starts the node and waits up to 10 s for it to report online.
pub fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if !n.status().online {
        // Said in the log, so a later step's error (an add_contact that fails) has its cause.
        eprintln!("online: no relay after 10 s, going on");
    }
}

fn poll(every_ms: u64, secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(every_ms));
    }
    panic!("timed out waiting for {what}");
}

/// Polls `f` every 250 ms for up to `secs`, else panics.
pub fn eventually(secs: u64, what: &str, f: impl FnMut() -> bool) {
    poll(250, secs, what, f)
}

/// As `eventually`, polling every 200 ms (the files that used that interval keep it).
pub fn eventually_200ms(secs: u64, what: &str, f: impl FnMut() -> bool) {
    poll(200, secs, what, f)
}

/// Polls `f` every 200 ms for up to `secs`: whether it ever held (bounded "does not happen" claims).
pub fn within(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    f()
}

/// Unix time in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Unix time in milliseconds, signed (for the files whose arithmetic is `i64`).
pub fn now_ms_i64() -> i64 {
    now_ms() as i64
}

/// Unix time in seconds.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
