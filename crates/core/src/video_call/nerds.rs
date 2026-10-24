//! "Stats for nerds": the sheet's rows and their text live in `video_call_core::nerds`; this
//! declares the row to UniFFI and exports the one function the apps call.
use super::*;

/// One row of the sheet: `section` "send", "recv" or "link" (the apps group by it).
#[uniffi::remote(Record)]
pub struct VideoStatRow {
    pub section: String,
    pub label: String,
    pub value: String,
}

/// The sheet's rows from one `video_stats()` read and the call's `rtt_ms` / `loss_pct` (the app
/// has them from `call_stats()`). Always 11 rows, same labels, same order.
#[uniffi::export]
pub fn video_stat_rows(stats: VideoStats, rtt_ms: u32, loss_pct: f64) -> Vec<VideoStatRow> {
    video_call_core::nerds::stat_rows(stats, rtt_ms, loss_pct)
}
