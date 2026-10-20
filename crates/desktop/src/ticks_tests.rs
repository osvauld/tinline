use super::*;

#[test]
fn transitions_are_one_shot_and_end_at_the_target_shape() {
    for (from, to) in [(Vis::Slack, Vis::One), (Vis::One, Vis::Two), (Vis::Two, Vis::Read), (Vis::Slack, Vis::Bad), (Vis::Slack, Vis::Two)] {
        assert!(pose(from, to, 0.0).is_some(), "{from:?} -> {to:?} animates");
        assert!(pose(from, to, 3.0).is_none(), "{from:?} -> {to:?} settles (no loop)");
    }
    assert!(pose(Vis::Two, Vis::Two, 0.0).is_none(), "no change, no animation");
    // Slack -> one starts slack and ends taut; read keeps both strings.
    let p = pose(Vis::Slack, Vis::One, 0.0).unwrap();
    assert!(p.k0 < 0.3 && p.k1.is_none());
    assert_eq!(rest(Vis::Read), Pose { k0: 1.0, k1: Some((1.0, 1.0)), mix: 1.0 });
    // Waiting -> not delivered: colour only, the shape stays slack.
    let b = pose(Vis::Slack, Vis::Bad, 0.1).unwrap();
    assert_eq!((b.k0, b.k1), (0.0, None));
    assert!(b.mix > 0.0 && b.mix < 1.0);
}

#[test]
fn every_state_draws_the_same_path_commands() {
    let cmds = |s: String| s.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>();
    assert_eq!(cmds(path(0.0, 0.0)), cmds(path(1.0, g::DX)));
    assert_eq!(cmds(path(0.0, 0.0)).len(), g::N);
}
