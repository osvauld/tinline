//! Startup timing: with `P2P_STARTUP_MARKS=1`, stderr gets `startup: <step> <ms>` lines, in ms
//! since `main` began, so a script can time a cold start without a profiler.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static ON: OnceLock<bool> = OnceLock::new();

/// Called first thing in `main`: the zero point.
pub fn begin() {
    START.get_or_init(Instant::now);
    ON.get_or_init(|| std::env::var("P2P_STARTUP_MARKS").is_ok_and(|v| v == "1"));
}

pub fn mark(step: &str) {
    if *ON.get().unwrap_or(&false) {
        let ms = START.get().map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0);
        eprintln!("startup: {step} {ms:.1}");
    }
}

/// [`mark`] only the first time `flag` is seen (per process).
pub fn mark_once(flag: &AtomicBool, step: &str) {
    if !flag.swap(true, Ordering::Relaxed) {
        mark(step);
    }
}
