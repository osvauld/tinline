//! Shared helpers, compiled once by the integration harness.
//! Group-call capacity tests remain nested under `group_call`.
#![allow(dead_code, unused_imports)]

pub mod calls;
pub mod chat;
pub mod codec;
pub mod events;
pub mod fs;
pub mod multi;
pub mod group;
pub mod peer;
pub mod tempdir;
pub mod wait;

pub use calls::{ended, ended_after, expect, incoming, saw_incoming};
pub use chat::{all_msgs, all_msgs_oldest_first, delivered, has, holds, ticks};
pub use events::{Ev, LEv, LTap, NoLink, Quiet, Tap};
pub use group::{Member, WAIT};
pub use peer::{LinkAs, Peer, Suite};
pub use tempdir::TempDir;
pub use wait::{eventually, eventually_200ms, now_ms, now_ms_i64, now_secs, online, within};

/// The passphrase every test identity uses.
pub const PASS: &str = "test-passphrase";

/// Run a test with process-local configuration set before any runtime threads exist.
/// Returns true in the child, false after the parent has checked the child's result.
/// Using a child of this same executable keeps one link while preserving isolation.
pub fn with_env(vars: &[(&str, &str)]) -> bool {
    let thread = std::thread::current();
    let name = thread.name().expect("libtest names each test thread");
    const CHILD: &str = "P2PCORE_IT_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        return true;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD, name)
        .envs(vars.iter().copied())
        .output()
        .expect("start isolated integration test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "isolated test {name} failed: {}\n{}\n{}",
        output.status,
        stdout,
        String::from_utf8_lossy(&output.stderr),
    );
    false
}
