use super::*;
#[test]
fn presentation_resets_on_new_call_not_timer_updates() {
    let mut s = State::default(); s.bind(Some("a"));
    s.apply(Action::Minimize, true); s.bind(Some("a")); assert!(s.minimized);
    s.apply(Action::About, true); s.bind(Some("b")); assert!(!s.about && !s.minimized);
    s.bind(None); s.apply(Action::About, true); assert!(!s.about);
}
#[test]
fn incoming_cannot_minimize_and_details_close_independently() {
    let mut s = State::default(); s.bind(Some("a"));
    s.apply(Action::Minimize, false); assert!(!s.minimized);
    s.apply(Action::About, false); s.apply(Action::CloseAbout, false); assert!(!s.about);
}
#[test]
fn signal_uses_recent_loss_and_shared_thresholds() {
    let mut l = Loss::default();
    l.push(0, 0); l.push(1000, 200);           // a bad first second
    for i in 2..9 { l.push(1000 * i, 200); }  // then clean
    assert_eq!(l.pct(), 0.0);
    assert_eq!([strength(42, 0.2), strength(140, 1.4), strength(280, 5.0), strength(520, 14.0)], [4, 3, 2, 1]);
    assert_eq!(signal_word(strength(140, 1.4)), "Good");
}
#[test]
fn fixed_boxes_match_the_playground_flex_equations() {
    let g = Geometry::new(800.0);
    assert_eq!(g, Geometry { avatar_top: 209.0, controls_top: 674.0, text_top: 359.0 });
    assert_eq!(g.text_top - g.avatar_top, 150.0);
}
