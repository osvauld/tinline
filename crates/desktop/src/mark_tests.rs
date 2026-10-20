use super::*;

/// Numeric arc length of the drawn string for a pose.
fn drawn_len(p: &Params, pose: Pose) -> f64 {
    let x1 = p.c - pose.delta + p.ko;
    let x2 = 24.0 - x1;
    let (cx, cy) = (12.0 + 2.0 * pose.sag * pose.phi.sin(), 12.0 + 2.0 * pose.sag * pose.phi.cos());
    let at = |t: f64| {
        let u = 1.0 - t;
        (u * u * x1 + 2.0 * u * t * cx + t * t * x2, u * u * 12.0 + 2.0 * u * t * cy + t * t * 12.0)
    };
    let n = 4000;
    let mut len = 0.0;
    let mut prev = at(0.0);
    for i in 1..=n {
        let q = at(i as f64 / n as f64);
        len += ((q.0 - prev.0).powi(2) + (q.1 - prev.1).powi(2)).sqrt();
        prev = q;
    }
    len
}

#[test]
fn string_length_is_conserved_while_the_cans_move() {
    for px in [24.0, 28.0, 32.0, 48.0, 128.0] {
        let p = Params::tier(px);
        for i in 0..=10 {
            let m = Model { d: p.dt * i as f64 / 10.0, ..Model::new(p) };
            let pose = Pose { delta: m.d, sag: m.sag(), phi: 0.0 };
            assert!((drawn_len(&p, pose) - p.ell).abs() < 1e-3, "tier {px} d={}", m.d);
        }
    }
}

/// Drives a connect at 60 fps; returns (tug times in s, settle time in s).
fn pull(m: &mut Model) -> (Vec<f64>, f64) {
    m.connect();
    let (mut t, mut tugs, mut seen) = (0.0, Vec::new(), m.tug_count);
    while m.advance(1.0 / 600.0) {
        t += 1.0 / 600.0;
        if m.tug_count > seen {
            seen = m.tug_count;
            tugs.push(t);
        }
        assert!(t < 3.0, "still moving after 3 s");
    }
    (tugs, t)
}

#[test]
fn connect_pulls_taut_once_and_settles() {
    // The everyday pull (MARK-MATH 2026-10-13): arrives with speed, recoils off the tight
    // string, bounces a few times smaller each time, and rests taut.
    let mut m = Model::new(Params::tier(96.0));
    let (tugs, settled) = pull(&mut m);
    assert!(tugs.len() >= 2, "the cans bounce back off the string at least once, got {}", tugs.len());
    assert!(settled < 1.2, "settles about 770 ms after connect, got {settled:.3}");
    assert_eq!(m.d, m.p.dt);
    assert!(m.pose().sag.abs() < 1e-6, "taut string has no sag");
    assert!(!m.active());
}

#[test]
fn line_spring_tugs_twice_on_the_cue_gap_and_rests() {
    let mut m = Model::new(Params::tier(96.0));
    m.set_spring(Spring::line());
    let mut give: f64 = 0.0;
    m.connect();
    let (mut t, mut tugs, mut seen) = (0.0, Vec::new(), 0);
    while m.advance(1.0 / 600.0) {
        t += 1.0 / 600.0;
        if m.tug_count > seen {
            seen = m.tug_count;
            tugs.push(t);
        }
        if tugs.len() == 1 {
            give = give.max(m.p.dt - m.d);
        }
        assert!(t < 3.0);
    }
    assert_eq!(tugs.len(), 2, "exactly two tugs");
    let gap = tugs[1] - tugs[0];
    assert!((0.10..0.16).contains(&gap), "the tugs land ~130 ms apart (cue gap 120 ms), got {gap:.3}");
    // about 1.6 px at 96 between the tugs (grid units x 4 px)
    let px = give * 96.0 / 24.0;
    assert!((1.0..2.4).contains(&px), "the line gives about 1.6 px, got {px:.2}");
    assert!(m.tight && m.d == m.p.dt && !m.active());
}

#[test]
fn reduced_motion_line_still_counts_one_tug() {
    let mut m = Model::new(Params::tier(96.0));
    m.set_spring(Spring::line());
    m.reduced = true;
    m.connect();
    assert_eq!(m.tug_count, 1, "the colour warms and the cue plays");
    assert!(m.tight && m.d == m.p.dt);
}

#[test]
fn sway_runs_until_ended_then_comes_to_rest() {
    let mut m = Model::new(Params::tier(28.0));
    m.ring(true);
    let mut peak: f64 = 0.0;
    for _ in 0..300 {
        assert!(m.advance(1.0 / 60.0), "ringing never idles");
        peak = peak.max(m.phi.abs());
    }
    assert!((peak - rad(PHI0_DEG)).abs() < rad(1.0), "sways to the amplitude, got {}", peak.to_degrees());
    m.ring(false);
    m.end();
    let mut t = 0.0;
    while m.advance(1.0 / 60.0) {
        t += 1.0 / 60.0;
        assert!(t < 4.0);
    }
    assert_eq!((m.d, m.phi), (0.0, 0.0));
}

#[test]
fn end_returns_a_taut_string_to_slack() {
    let mut m = Model::new(Params::tier(64.0));
    m.set(true);
    m.end();
    let mut t = 0.0;
    while m.advance(1.0 / 60.0) {
        t += 1.0 / 60.0;
        assert!(t < 3.0);
    }
    assert_eq!(m.d, 0.0);
    assert!((m.pose().sag - m.p.s0).abs() < 1e-9);
}

#[test]
fn pluck_is_quiet_and_does_not_stack() {
    let mut m = Model::new(Params::tier(28.0));
    m.set(true);
    m.pluck();
    assert_eq!(m.tw_a, PLUCK_A);
    let before = m.tw;
    m.advance(0.01);
    m.pluck();
    assert!(m.tw > before, "a running twang is left alone");
    let mut t = 0.0;
    while m.advance(1.0 / 60.0) {
        t += 1.0 / 60.0;
        assert!(t < 2.0);
    }
    // slack: a small swing that dies out
    let mut s = Model::new(Params::tier(28.0));
    s.pluck();
    assert!(s.advance(1.0 / 60.0));
    while s.advance(1.0 / 60.0) {}
    assert_eq!(s.phi, 0.0);
}

#[test]
fn reduced_motion_jumps_to_end_poses() {
    let mut m = Model::new(Params::tier(48.0));
    m.reduced = true;
    m.connect();
    assert!(!m.advance(1.0 / 60.0));
    assert_eq!(m.d, m.p.dt);
    m.ring(true);
    assert!(!m.ring && m.d == 0.0);
    m.pluck();
    assert!(!m.active());
}

#[test]
fn small_tiers_stay_put_and_only_the_sag_changes() {
    let mut m = Model::new(Params::tier(16.0));
    assert!(m.p.sag_only);
    let slack = m.pose();
    m.set(true);
    let taut = m.pose();
    assert_eq!((slack.delta, taut.delta), (0.0, 0.0));
    assert!(taut.sag < slack.sag);
}

#[test]
fn status_look_matches_the_playground_table() {
    let l = status_look(Some(CallPhase::Incoming), Net::Online);
    assert_eq!((l.state, l.sway, l.dim), (MarkState::Ringing, Some((1.6, 12.0)), false));
    assert_eq!(status_look(Some(CallPhase::Outgoing), Net::Offline).sway, Some((2.0, 9.0)));
    assert_eq!(status_look(Some(CallPhase::Active), Net::Offline).state, MarkState::Taut);
    assert_eq!(status_look(Some(CallPhase::Reconnecting), Net::Online).sway, Some((2.0, 4.0)));
    assert_eq!(status_look(None, Net::Connecting).sway, Some((3.2, 5.0)));
    let off = status_look(None, Net::Offline);
    assert_eq!((off.state, off.dim), (MarkState::Slack, true));
    assert_eq!(status_look(None, Net::Online).state, MarkState::Taut);
}

/// Drives the widget's update() with redraw events 16 ms apart, as the window would.
struct Rig {
    st: Live,
    t: Instant,
    size: f32,
}
impl Rig {
    fn new(size: f32) -> Rig {
        Rig { st: Live::default(), t: Instant::now(), size }
    }
    /// One frame; true when the widget asked for another one.
    fn frame(&mut self, look: Look) -> bool {
        let prog = Prog::<()> { size: self.size, look, can: Color::BLACK, string: Color::BLACK, slack: Color::BLACK, on_tug: None };
        self.t += std::time::Duration::from_millis(16);
        let ev = iced::Event::Window(window::Event::RedrawRequested(self.t));
        let a = <Prog<()> as canvas::Program<()>>::update(&prog, &mut self.st, &ev, Rectangle::with_size(iced::Size::new(self.size, self.size)), mouse::Cursor::Unavailable);
        a.is_some()
    }
    fn model(&self) -> &Model {
        self.st.model.as_ref().unwrap()
    }
}

#[test]
fn widget_ringing_keeps_asking_for_frames_and_the_string_moves() {
    let mut r = Rig::new(28.0);
    let look = Look { state: MarkState::Ringing, sway: Some((1.6, 12.0)), ..Look::default() };
    let mut phis = std::collections::BTreeSet::new();
    for _ in 0..100 {
        assert!(r.frame(look), "ringing must request the next frame");
        phis.insert((r.model().pose().phi * 1e4) as i64);
    }
    assert!(phis.len() > 50, "the swing angle changes frame to frame");
    assert!(r.model().pose().phi.abs() > 0.0);
}

#[test]
fn widget_connect_twangs_once_then_goes_quiet_and_send_plucks() {
    let mut r = Rig::new(28.0);
    let slack = Look::default();
    assert!(!r.frame(slack), "a still mark asks for nothing");
    let taut = Look { state: MarkState::Taut, ..Look::default() };
    let mut frames = 0;
    let mut twanged = false;
    while r.frame(taut) {
        frames += 1;
        twanged |= r.model().tw >= 0.0;
        assert!(frames < 400);
    }
    assert!(twanged, "the string twangs when it goes tight");
    assert!(r.model().hit && r.model().tug_count >= 1);
    assert_eq!(r.model().pose().delta, r.model().params().dt);
    assert!(!r.frame(taut), "still again: no more frames");
    // send: pluck counter changes -> a small twang, then quiet
    let plucked = Look { pluck: 1, ..taut };
    assert!(r.frame(plucked));
    assert_eq!(r.model().tw_a, PLUCK_A);
    let mut n = 0;
    while r.frame(plucked) {
        n += 1;
        assert!(n < 100);
    }
    assert!(!r.frame(plucked));
    // and on a slack mark a pluck is a little swing
    let mut s = Rig::new(28.0);
    assert!(!s.frame(slack));
    assert!(s.frame(Look { pluck: 1, ..slack }));
    assert!(s.model().phv.abs() > 0.0);
}

#[test]
fn widget_reports_each_tug_and_warms_the_line_but_not_when_mounted_taut() {
    let drive = |start: MarkState, end: MarkState| -> (usize, f64) {
        let mut st = Live::default();
        let mut t = Instant::now();
        let mut tugs = 0;
        let look = |state| Look { state, spring: Spring::line(), line_color: true, ..Look::default() };
        for i in 0..200 {
            let prog = Prog::<u8> { size: 96.0, look: look(if i == 0 { start } else { end }), can: Color::BLACK, string: Color::BLACK, slack: Color::WHITE, on_tug: Some(7) };
            t += std::time::Duration::from_millis(16);
            let ev = iced::Event::Window(window::Event::RedrawRequested(t));
            if let Some(a) = <Prog<u8> as canvas::Program<u8>>::update(&prog, &mut st, &ev, Rectangle::with_size(iced::Size::new(96.0, 96.0)), mouse::Cursor::Unavailable) {
                if a.into_inner().0 == Some(7) {
                    tugs += 1;
                }
            }
        }
        (tugs, st.warm)
    };
    let (tugs, warm) = drive(MarkState::Slack, MarkState::Taut);
    assert_eq!((tugs, warm), (2, 1.0), "two tugs reported, the line fully warm");
    let (tugs, warm) = drive(MarkState::Taut, MarkState::Taut);
    assert_eq!((tugs, warm), (0, 1.0), "mounted taut: no replayed tug or cue");
}

#[test]
fn tier_picking_matches_the_table() {
    assert_eq!(pick_tier(28.0), 28);
    assert_eq!(pick_tier(30.0), 28);
    assert_eq!(pick_tier(48.0), 48);
    assert_eq!(pick_tier(44.0), 40);
    assert_eq!(pick_tier(18.0), 16);
}
