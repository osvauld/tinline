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
