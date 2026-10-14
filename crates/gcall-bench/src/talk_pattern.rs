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
mod tests {
    use super::*;

    #[test]
    fn turn_taking_has_one_speaker_or_silence() {
        for n in [2, 11, 15, 23, 31] {
            for tick in 0..n as u64 * 120 {
                let active: Vec<_> = (1..=n)
                    .filter(|&s| TalkPattern::TurnTaking.talking(s, n, tick))
                    .collect();
                if tick % 60 < 50 {
                    assert_eq!(active, vec![((tick / 60) % n as u64 + 1) as u8]);
                } else {
                    assert!(active.is_empty());
                }
            }
        }
    }

    #[test]
    fn overlap_is_everyone_not_a_reduced_demand_denominator() {
        for tick in 0..120 {
            for slot in 1..=31 {
                assert_eq!(TalkPattern::Overlap.talking(slot, 31, tick), tick % 60 < 50);
            }
        }
    }
}
