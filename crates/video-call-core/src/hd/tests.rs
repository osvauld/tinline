use super::*;

const P480: u64 = 640 * 480;
const P540: u64 = 960 * 540;
const P720: u64 = 1280 * 720;

/// A good line with HD off: 640x480 captured, the ladder at 720p, 2 Mbps, a 6 ms encoder.
fn good() -> SendSample {
    SendSample {
        hd_on: false,
        frames: 30,
        send_px: P480,
        rung_px: P720,
        cap_px: P720,
        rate_bps: 2_000_000,
        congested: false,
        cpu_step: false,
        encode_p75_us: Some(6_000),
        fps: 30,
        hw: false,
    }
}

/// Feeds `samples` one per second; returns the second (1-based) the rule first says "room".
fn first_room(samples: impl IntoIterator<Item = SendSample>) -> Option<u32> {
    let mut st = Streak::default();
    samples
        .into_iter()
        .zip(1..)
        .find(|(s, _)| st.observe(send_ok(s), ON_SECS))
        .map(|(_, i)| i)
}

#[test]
fn video_call_hd_good_line_has_room_after_on_secs() {
    assert_eq!(first_room(std::iter::repeat_n(good(), 30)), Some(ON_SECS));
    // Right at the threshold counts; a hardware encoder needs no time.
    let edge = SendSample {
        rate_bps: ROOM_BPS,
        encode_p75_us: None,
        hw: true,
        ..good()
    };
    assert_eq!(first_room(std::iter::repeat_n(edge, 30)), Some(ON_SECS));
}

#[test]
fn video_call_hd_weak_or_borderline_line_never() {
    // Below the 720p threshold for the whole call.
    let weak = SendSample {
        rate_bps: 1_100_000,
        ..good()
    };
    assert_eq!(first_room(std::iter::repeat_n(weak, 120)), None);
    // Borderline: above it, but a dip every 5 s.
    let calls = (0..120).map(|i| if i % 5 == 4 { weak } else { good() });
    assert_eq!(first_room(calls), None);
    // The ladder isn't at 540p or above (the rate never got there): no room either.
    assert_eq!(
        first_room(std::iter::repeat_n(
            SendSample {
                rung_px: 640 * 360,
                send_px: 480 * 360,
                ..good()
            },
            60
        )),
        None
    );
}

#[test]
fn video_call_hd_overuse_resets() {
    let over = SendSample {
        congested: true,
        ..good()
    };
    // 4 good seconds, overuse, then it takes another full 5.
    let s = std::iter::repeat_n(good(), 4)
        .chain([over])
        .chain(std::iter::repeat_n(good(), 30));
    assert_eq!(first_room(s), Some(4 + 1 + ON_SECS));
    // Room already earned goes at once with an overuse, and is earned again from zero.
    let mut st = Streak::default();
    for _ in 0..ON_SECS {
        st.observe(send_ok(&good()), ON_SECS);
    }
    assert!(st.observe(send_ok(&good()), ON_SECS));
    assert!(!st.observe(send_ok(&over), ON_SECS));
    assert!(!st.observe(send_ok(&good()), ON_SECS));
    // A CPU step down resets it the same way.
    let cpu = SendSample {
        cpu_step: true,
        ..good()
    };
    let s = std::iter::repeat_n(good(), 4)
        .chain([cpu])
        .chain(std::iter::repeat_n(good(), 30));
    assert_eq!(first_room(s), Some(4 + 1 + ON_SECS));
}

#[test]
fn video_call_hd_already_on_never() {
    assert_eq!(
        first_room(std::iter::repeat_n(
            SendSample {
                hd_on: true,
                ..good()
            },
            60
        )),
        None
    );
}

#[test]
fn video_call_hd_not_when_something_else_holds_it_back() {
    let none = |s: SendSample| first_room(std::iter::repeat_n(s, 60));
    // Already sending a sharper picture (the capture isn't the limit).
    assert_eq!(
        none(SendSample {
            send_px: P720,
            ..good()
        }),
        None
    );
    // The peer's screen (or offer, or my battery) caps it below 540p.
    assert_eq!(
        none(SendSample {
            cap_px: 640 * 360,
            ..good()
        }),
        None
    );
    // An encoder that would not make 540p in time: 640x480 at 12 ms -> 540p ~20.3 ms > 20.
    assert_eq!(
        none(SendSample {
            encode_p75_us: Some(12_000),
            ..good()
        }),
        None
    );
    // No encode times yet (a fresh encoder).
    assert_eq!(
        none(SendSample {
            encode_p75_us: None,
            ..good()
        }),
        None
    );
    // Camera off: nothing sent.
    assert_eq!(
        none(SendSample {
            frames: 0,
            ..good()
        }),
        None
    );
}

fn big() -> RecvSample {
    RecvSample {
        wanted_px: P720,
        limited: false,
        frames: 30,
        recv_px: P480,
        cost_ms: 4.0,
        cost_n: 60,
        hw: false,
        behind: false,
    }
}

#[test]
fn video_call_hd_receiver_room() {
    assert!(recv_ok(&big()));
    // Drawn small (asks for 360p): no room to offer.
    assert!(!recv_ok(&RecvSample {
        wanted_px: 640 * 360,
        ..big()
    }));
    // Already getting 540p.
    assert!(!recv_ok(&RecvSample {
        recv_px: P540,
        ..big()
    }));
    // Battery saver / heat / a slow decoder holds my hint down.
    assert!(!recv_ok(&RecvSample {
        limited: true,
        ..big()
    }));
    // Decode + render 12 ms at 640x480 -> ~20.3 ms at 540p: over the 20 ms budget.
    assert!(!recv_ok(&RecvSample {
        cost_ms: 12.0,
        ..big()
    }));
    assert!(
        !recv_ok(&RecvSample { cost_n: 3, ..big() }),
        "too few samples"
    );
    // Hardware: throughput, not time.
    assert!(recv_ok(&RecvSample {
        hw: true,
        cost_ms: 30.0,
        ..big()
    }));
    assert!(!recv_ok(&RecvSample {
        hw: true,
        behind: true,
        ..big()
    }));
    // Hidden or no frames.
    assert!(!recv_ok(&RecvSample { frames: 0, ..big() }));
}

/// Feeds `samples` one per second; returns the seconds (1-based) at which the choice changed.
fn switches(samples: impl IntoIterator<Item = SendSample>) -> Vec<u32> {
    let mut auto = Auto::default();
    let mut was = false;
    let mut out = vec![];
    for (s, i) in samples.into_iter().zip(1..) {
        // The app follows: once HD, the camera captures HD and sends the rung's size.
        let s = if was { SendSample { hd_on: true, send_px: s.rung_px.min(P720), ..s } } else { s };
        let on = auto.observe(&s);
        if on != was {
            out.push(i);
        }
        was = on;
    }
    out
}

#[test]
fn video_call_hd_auto_on_after_on_secs_and_stays_through_a_dip() {
    assert_eq!(switches(std::iter::repeat_n(good(), 60)), [ON_SECS]);
    // Once HD, a 10 s dip to 360p keeps the camera (a reopen costs a freeze).
    let dip = SendSample { rung_px: 640 * 360, rate_bps: 500_000, congested: true, ..good() };
    let s = std::iter::repeat_n(good(), 10)
        .chain(std::iter::repeat_n(dip, OFF_SECS as usize - 1))
        .chain(std::iter::repeat_n(good(), 30));
    assert_eq!(switches(s), [ON_SECS]);
    // Overuse or a CPU step while HD is on doesn't turn it off by itself either.
    let s = std::iter::repeat_n(good(), 10).chain([SendSample { congested: true, cpu_step: true, ..good() }]).chain(std::iter::repeat_n(good(), 10));
    assert_eq!(switches(s), [ON_SECS]);
}

#[test]
fn video_call_hd_auto_off_after_off_secs_below_540p_then_on_again() {
    let low = SendSample { rung_px: 640 * 360, rate_bps: 500_000, ..good() };
    let s = std::iter::repeat_n(good(), 10)
        .chain(std::iter::repeat_n(low, 30))
        .chain(std::iter::repeat_n(good(), 30));
    assert_eq!(switches(s), [ON_SECS, 10 + OFF_SECS, 40 + ON_SECS]);
    // The peer's picture shrinks below 540p (a small window, a hidden picture): off as well.
    let small = SendSample { cap_px: 640 * 360, ..good() };
    let s = std::iter::repeat_n(good(), 10).chain(std::iter::repeat_n(small, 30));
    assert_eq!(switches(s), [ON_SECS, 10 + OFF_SECS]);
    // The camera reopening (no frames) doesn't count toward off.
    let none = SendSample { frames: 0, rung_px: 640 * 360, ..good() };
    let s = std::iter::repeat_n(good(), 10).chain(std::iter::repeat_n(none, 60));
    assert_eq!(switches(s), [ON_SECS]);
}

#[test]
fn video_call_hd_auto_never_on_a_weak_line() {
    let weak = SendSample { rate_bps: 1_100_000, ..good() };
    assert_eq!(switches(std::iter::repeat_n(weak, 120)), Vec::<u32>::new());
}
