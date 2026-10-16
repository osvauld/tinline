//! Receipt tick state-change rules, shared by both apps (UniFFI) so desktop and Android never drift:
//! which visual state a message's mark shows, and which one-shot morph a state change plays.
//!
//! - slack -> one: the slack string is pulled taut (`Pull`, string 1);
//! - one -> two: the second string pulls out of the first, sliding to its offset as it goes taut (`Pull`);
//! - slack -> two: both strings pull taut together (`Pull`);
//! - two -> read: both strings pluck once (`Pluck`) while the colour eases to the read ink;
//! - any change of ink (read, not delivered) eases the colour; waiting -> not delivered is colour only.
//!
//! Apps keep what is theirs: sampling the generated curves (crates/tin-icons `PULL` / `PLUCK` at
//! `FRAME_HZ`), the colour ease (their motion token), reduced motion, and drawing. Nothing loops.

use super::api::ReceiptTick;

/// What a message's mark shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum TickVis {
    /// Waiting to send / waiting for a host: the slack string.
    Slack,
    /// Left this device (1:1: sent, not confirmed yet; groups: sent, not every member confirmed).
    One,
    /// Every recipient account signed that it holds it.
    Two,
    /// Every recipient account signed that it was read.
    Read,
    /// Not delivered (a block stopped it): the slack string in the danger ink.
    Bad,
}

/// Which generated curve a transition samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TickCurve {
    /// No shape change.
    None,
    /// `PULL`: a string pulled taut.
    Pull,
    /// `PLUCK`: both strings plucked once.
    Pluck,
}

/// The one-shot transition between two states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TickTransition {
    pub curve: TickCurve,
    /// The ink changes (eased with the app's motion token).
    pub colour: bool,
}

/// One frame's shape: string 1 morph (0 slack .. 1 taut), and string 2 (shown if `second`) morph and
/// offset fraction (of `DX`).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct TickPose {
    pub k0: f32,
    pub second: bool,
    pub k1: f32,
    pub k1_offset: f32,
}

fn shape(v: TickVis) -> u8 {
    match v {
        TickVis::Slack | TickVis::Bad => 0,
        TickVis::One => 1,
        TickVis::Two | TickVis::Read => 2,
    }
}

fn tone(v: TickVis) -> u8 {
    match v {
        TickVis::Read => 1,
        TickVis::Bad => 2,
        _ => 0,
    }
}

/// The mark of my message: the core's `ReceiptTick` (None while it is not loaded), or not delivered.
/// `None` = no mark (incoming, deleted, notes to self, not known yet).
#[uniffi::export]
pub fn tick_vis(tick: Option<ReceiptTick>, not_delivered: bool) -> Option<TickVis> {
    if not_delivered {
        return Some(TickVis::Bad);
    }
    match tick? {
        ReceiptTick::OnlyHere => Some(TickVis::Slack),
        ReceiptTick::Waiting => Some(TickVis::One),
        ReceiptTick::Delivered => Some(TickVis::Two),
        ReceiptTick::Read => Some(TickVis::Read),
        ReceiptTick::None => None,
    }
}

/// What a change from `from` to `to` plays.
#[uniffi::export]
pub fn tick_transition(from: TickVis, to: TickVis) -> TickTransition {
    let curve = match (shape(from), shape(to)) {
        (0, 1) | (1, 2) | (0, 2) => TickCurve::Pull,
        (2, 2) if to == TickVis::Read && from != TickVis::Read => TickCurve::Pluck,
        _ => TickCurve::None,
    };
    TickTransition {
        curve,
        colour: tone(from) != tone(to),
    }
}

/// The shape of a `from` -> `to` transition at curve sample `x` (the value of the curve
/// `tick_transition` named, at the frame's index); `None` = settled: the rest shape of `to`.
#[uniffi::export]
pub fn tick_pose(from: TickVis, to: TickVis, x: Option<f32>) -> TickPose {
    let mut p = match shape(to) {
        0 => TickPose {
            k0: 0.0,
            second: false,
            k1: 0.0,
            k1_offset: 0.0,
        },
        1 => TickPose {
            k0: 1.0,
            second: false,
            k1: 0.0,
            k1_offset: 0.0,
        },
        _ => TickPose {
            k0: 1.0,
            second: true,
            k1: 1.0,
            k1_offset: 1.0,
        },
    };
    let Some(x) = x else { return p };
    match (tick_transition(from, to).curve, shape(from), shape(to)) {
        (TickCurve::Pull, 0, 1) => p.k0 = x,
        // The second string pulls out of the first as it goes taut.
        (TickCurve::Pull, 1, 2) => {
            p.k1 = 0.5 + 0.5 * x;
            p.k1_offset = x.max(0.0);
        }
        // Straight from waiting: both pull taut together.
        (TickCurve::Pull, 0, 2) | (TickCurve::Pluck, _, _) => {
            p.k0 = x;
            p.k1 = x;
            p.k1_offset = 1.0;
        }
        _ => {}
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use TickVis::*;

    #[test]
    fn transitions_name_the_same_morphs_as_both_apps_did() {
        let t = |a, b| tick_transition(a, b);
        assert_eq!(
            t(Slack, One),
            TickTransition {
                curve: TickCurve::Pull,
                colour: false
            }
        );
        assert_eq!(t(One, Two).curve, TickCurve::Pull);
        assert_eq!(t(Slack, Two).curve, TickCurve::Pull);
        assert_eq!(
            t(Two, Read),
            TickTransition {
                curve: TickCurve::Pluck,
                colour: true
            }
        );
        assert_eq!(
            t(Slack, Bad),
            TickTransition {
                curve: TickCurve::None,
                colour: true
            }
        );
        assert_eq!(
            t(Two, Two),
            TickTransition {
                curve: TickCurve::None,
                colour: false
            }
        );
        assert_eq!(t(Read, Read).curve, TickCurve::None);
    }

    #[test]
    fn poses_match_the_former_app_rules() {
        assert_eq!(
            tick_pose(Slack, One, Some(0.2)),
            TickPose {
                k0: 0.2,
                second: false,
                k1: 0.0,
                k1_offset: 0.0
            }
        );
        assert_eq!(
            tick_pose(One, Two, Some(0.0)),
            TickPose {
                k0: 1.0,
                second: true,
                k1: 0.5,
                k1_offset: 0.0
            }
        );
        assert_eq!(tick_pose(One, Two, Some(-0.2)).k1_offset, 0.0);
        assert_eq!(
            tick_pose(Two, Read, Some(0.9)),
            TickPose {
                k0: 0.9,
                second: true,
                k1: 0.9,
                k1_offset: 1.0
            }
        );
        assert_eq!(tick_pose(Slack, Bad, Some(0.5)), tick_pose(Bad, Bad, None));
        assert_eq!(
            tick_pose(Read, Read, None),
            TickPose {
                k0: 1.0,
                second: true,
                k1: 1.0,
                k1_offset: 1.0
            }
        );
    }

    #[test]
    fn not_delivered_wins_and_none_means_no_mark() {
        assert_eq!(tick_vis(Some(ReceiptTick::Read), true), Some(Bad));
        assert_eq!(tick_vis(None, true), Some(Bad));
        assert_eq!(tick_vis(None, false), None);
        assert_eq!(tick_vis(Some(ReceiptTick::None), false), None);
        assert_eq!(tick_vis(Some(ReceiptTick::Waiting), false), Some(One));
    }
}
