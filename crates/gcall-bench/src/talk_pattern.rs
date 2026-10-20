//! Deterministic experiment schedules, not product speaker/admission policy.
//! Times are member-process frame ticks; host camera/audio is excluded from these schedules.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TalkPattern {
    TurnTaking,
    Overlap,
}

impl TalkPattern {
    /// One-second utterance then 200 ms silence. Slots are 1..=members.
    pub fn talking(self, slot: u8, members: u8, tick: u64) -> bool {
        assert!(members > 0 && slot > 0 && slot <= members);
        let phase = tick % 60;
        match self {
            Self::TurnTaking => phase < 50 && (tick / 60) % members as u64 == slot as u64 - 1,
            Self::Overlap => phase < 50,
        }
    }
}

#[cfg(test)]
mod tests;
