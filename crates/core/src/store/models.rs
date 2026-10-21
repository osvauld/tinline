//! UniFFI's declaration of the call log record, whose model lives in `account_model::store`.
//! It stays in a module named `store::models` because UniFFI puts the declaring module's path into
//! the record's type metadata, and so into the checksums of every method that returns it; this
//! path keeps the generated bindings identical to the ones from before the split.

use account_model::store::CallRecord;

/// One finished call, as the UI lists it. `reason` is one of the stable end reasons in
/// `docs/protocol.md`.
#[uniffi::remote(Record)]
pub struct CallRecord {
    pub call_id: String,
    pub peer_did: String,
    /// Their name when the call happened (our alias for them if we had one).
    pub peer_name: String,
    pub incoming: bool,
    /// Unix seconds when the call started (dialing, or the first ring).
    pub started_at: u64,
    /// Seconds from answer to end; 0 if it never became active.
    pub duration_secs: u32,
    pub reason: String,
    /// Media went over a direct path rather than a relay (as last seen).
    pub direct: bool,
    /// Incoming and we never answered it (not one we declined, nor one turned away while
    /// unavailable).
    pub missed: bool,
    /// A video call: placed as one (`CallInfo.video`), or a camera was on while it was active.
    pub video: bool,
}
