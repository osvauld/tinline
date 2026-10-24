//! Application video payload counts, not QUIC traffic/ACK counters or configured bitrate.

/// The caller invokes this only at the stated admission/receive boundary.
/// Counters saturate rather than wrap; no packet header/QUIC overhead invented.
pub fn record(bytes: &mut u64, units: &mut u64, payload_len: usize) {
    *bytes = bytes.saturating_add(u64::try_from(payload_len).unwrap_or(u64::MAX));
    *units = units.saturating_add(1);
}

/// Per-second readout from measured counter deltas. No rate until a usable initial window.
pub fn payload_bps(bytes: u64, seconds: f32) -> u32 {
    if !seconds.is_finite() || seconds < 0.5 {
        return 0;
    }
    (bytes as f64 * 8.0 / seconds as f64).min(u32::MAX as f64) as u32
}

#[cfg(test)]
mod tests;
