//! The group call log: the runtime is `group_runtime::calllog`; this declares its record to
//! UniFFI and gives the apps its label.

pub use group_runtime::calllog::*;

// The record lives with its vault rows (`group_core::calllog`); this declares it to UniFFI.
// The record lives with its vault rows (`group_core::calllog`); this declares it to UniFFI.
/// A group call as the log lists it. Times are unix seconds like `CallRecord`.
#[uniffi::remote(Record)]
pub struct GroupCallRecord {
    pub call_id: String,
    pub group_id: String,
    /// DID of whoever started the call (empty when this device never learned it).
    pub started_by: String,
    /// When the call started (announce time), unix seconds.
    pub started_at: u64,
    /// Seconds this device was in the call (joining to leaving); for a missed call, announce to
    /// end.
    pub duration_secs: u32,
    /// The most people (distinct DIDs, this account included) seen in the call at once; 0 for a
    /// missed call, which this device never saw the roster of.
    pub people: u32,
    /// The call ended without this device ever joining it.
    pub missed: bool,
}

/// The line the apps show for a record (playground `callText` / `groupWhat`): "Group call · 19
/// min · 4 people", "Missed group call · 5 min" (calm, never an error).
#[uniffi::export]
pub fn group_call_label(r: GroupCallRecord) -> String {
    let s = r.duration_secs;
    let len = if s < 60 { format!("{s} s") } else { format!("{} min", (s + 30) / 60) };
    if r.missed {
        format!("Missed group call \u{b7} {len}")
    } else if r.people > 1 {
        format!("Group call \u{b7} {len} \u{b7} {} people", r.people)
    } else {
        format!("Group call \u{b7} {len}")
    }
}

#[cfg(test)]
mod tests;
