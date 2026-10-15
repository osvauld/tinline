//! Typed CALL OBSERVATIONS, not global attendance/read/media proofs. Runtime
//! hooks and proof/admission checks are intentionally separate from this codec.
use super::*;
pub(crate) const MAX_CALL_ID_BYTES: usize = 128; // provisional, real protocol ID checked by admission
pub(crate) const MAX_CALL_EVIDENCE: usize = 16; // exact supporting proof references per event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CallOutcome {
    Left,
    RemoteEnded,
    Removed,
    HostLost,
    Dropped,
    AnsweredElsewhere,
    Superseded,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CallEventKind {
    ObservedStart,
    Joined,
    ParticipationEnded { outcome: CallOutcome },
    Interrupted,
    ObservedHostEnd,
    Missed,
    Declined,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CallEvent {
    pub call_id: String,
    /// Host generation for group calls; individual calls have no group host.
    pub host_generation: Option<u32>,
    pub event: CallEventKind,
    /// Issuer's wall-clock observation, not trusted ordering/revocation time.
    pub observed_at_ms: u64,
    /// Locally measured segment duration only, not the group's global duration.
    pub active_ms: Option<u64>,
    /// Sorted distinct original protocol-proof digests. References alone do NOT
    /// establish that supporting proofs are present/verified; caller must check.
    pub evidence: Vec<[u8; 32]>,
}
impl CallEvent {
    pub(super) fn check(&self, scope: &Scope) -> Result<(), ReceiptError> {
        let at = i64::try_from(self.observed_at_ms).map_err(|_| ReceiptError::Bounds)?;
        if self.call_id.is_empty()
            || self.call_id.len() > MAX_CALL_ID_BYTES
            || !self.call_id.bytes().all(|b| b.is_ascii_graphic())
            || day_of(at) != scope.day
            || self.evidence.len() > MAX_CALL_EVIDENCE
            || !self.evidence.windows(2).all(|w| w[0] < w[1])
            || match scope.conversation {
                Conversation::Direct(_) => self.host_generation.is_some(),
                Conversation::Group(_) => !self.host_generation.is_some_and(|g| g > 0),
            }
            || (self.active_ms.is_some()
                && !matches!(
                    self.event,
                    CallEventKind::ParticipationEnded { .. } | CallEventKind::Interrupted
                ))
            || (matches!(
                self.event,
                CallEventKind::ObservedStart
                    | CallEventKind::Joined
                    | CallEventKind::ObservedHostEnd
                    | CallEventKind::Missed
                    | CallEventKind::Declined
            ) && self.evidence.is_empty())
            || (self.event == CallEventKind::ObservedHostEnd
                && matches!(scope.conversation, Conversation::Direct(_)))
        {
            return Err(ReceiptError::Bounds);
        }
        Ok(())
    }
}
#[cfg(test)]
pub(super) mod tests;
