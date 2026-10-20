use super::*;

#[test]
fn capture_preferences_keep_auto_and_bound_hd_fallback() {
    assert_eq!(Quality::default(), Quality::Auto);
    assert_eq!(Quality::Auto.sizes(), &[(640, 480)]);
    assert_eq!(Quality::PreferHd.sizes(), &[(1280, 720), (640, 480)]);
}

#[test]
fn call_choice_overrides_setting_only_for_the_start() {
    assert_eq!(quality_for(false, None), Quality::Auto);
    assert_eq!(quality_for(true, None), Quality::PreferHd);
    assert_eq!(quality_for(false, Some(true)), Quality::PreferHd);
    assert_eq!(quality_for(true, Some(false)), Quality::Auto);
}

#[test]
fn hd_paused_follows_actual_send_size() {
    assert!(!hd_paused(true, None));
    assert!(!hd_paused(true, Some((1280, 720))));
    assert!(hd_paused(true, Some((960, 540))));
    assert!(hd_paused(true, Some((540, 960))));
    assert!(!hd_paused(false, Some((320, 180))));
    assert_eq!(hd_line(true, true), "HD paused \u{b7} weak connection");
}

/// The real camera's start cost, the way a call starts it (`Camera::start_with_quality`): time to
/// streaming and to the first frame, for each quality. Needs a V4L2 camera and turns it on for a
/// moment; run by hand: `cargo test -p desktop --bin p2p-desktop camera::tests::real_camera_start -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_camera_start() {
    let src = find(None).expect("a V4L2 camera");
    for quality in [Quality::PreferHd, Quality::Auto, Quality::PreferHd] {
        let t0 = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx2 = tx.clone();
        let cam = Camera::start_with_quality(
            src.clone(),
            quality,
            move |r| {
                let _ = tx.send(format!("{:?} after {} ms", r.map(|o| (o.format, o.width, o.height)), t0.elapsed().as_millis()));
            },
            move |p| {
                let _ = tx2.send(format!("frame {}x{} after {} ms", p.width, p.height, t0.elapsed().as_millis()));
            },
        );
        let opened = rx.recv_timeout(Duration::from_secs(10)).expect("opened");
        let first = rx.recv_timeout(Duration::from_secs(10)).expect("first frame");
        let mut n = 0;
        let t1 = Instant::now();
        while t1.elapsed() < Duration::from_secs(1) {
            if rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                n += 1;
            }
        }
        println!("{quality:?}: {opened}; {first}; then {n} frames in 1 s");
        drop(cam);
        // The thread closes the device after its current frame.
        std::thread::sleep(Duration::from_millis(300));
    }
}
