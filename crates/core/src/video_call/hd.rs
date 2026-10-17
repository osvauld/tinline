//! "This call has room for sharper video": the rule behind the HD suggestion both apps show
//! (docs/video.md, "Suggesting sharper video"). Sharper video stays off by default; when the
//! line, the encoder and the peer's screen could all carry a sharper picture than the camera
//! gives (HD off captures 640x480), the call screen says so once and the person decides with the
//! HD chip. Nothing switches by itself. Mobile data is no exception.
//!
//! Two directions, one rule each, evaluated once a second by `adapt::run_policy`:
//! - **Sending** ([`send_ok`]): my HD is off and my picture is smaller than my ladder already
//!   allows, because of the capture, with the rate, the encoder and the peer's caps to spare.
//! - **Receiving** ([`recv_ok`]): I draw the peer's picture big, I get less than 540p, and my
//!   decoder would keep up at 540p. That sets [`video::media::Hint::room`], which tells the
//!   peer's sender; its own sending rule still decides whether its HD being off is the reason.
//!
//! Both have to hold for [`ROOM_SECS`] seconds in a row ([`Streak`]): a borderline link that
//! dips now and then never earns a suggestion.

use super::*;

/// The smallest picture sharper video brings on every device: 960x540 (phones aim for 540p,
/// computers for 720p). HD off sends at most 640x480.
pub(super) const SHARPER_PIXELS: u64 = 960 * 540;
/// The rate my line has to carry: the ladder's step-up threshold for 720p30 (floor 1 Mbps x
/// 130%), the point at which an HD capture would climb to the top rung. Above the 540p rung's
/// threshold (780 kbps) on purpose: a suggestion should lead to a clearly sharper picture, not to
/// one rung that a dip takes back.
pub(super) const ROOM_BPS: u32 = 1_300_000;
/// Seconds in a row the rule must hold: twice the ladder's 5 s hold for a step up, so the
/// suggestion never comes before the ladder itself would trust the rate.
pub(super) const ROOM_SECS: u32 = 10;
/// Encode / decode + render time budget, % of the frame interval: the ladder's and the decode
/// policy's step-down threshold (60%). A size that would be predicted above it is not "room".
const BUDGET_PCT: u64 = 60;

/// One second of my sending, as `run_policy` sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct SendSample {
    /// The app's HD choice for this call (`Node::set_video_hd`).
    pub hd_on: bool,
    /// Frames sent in the last second.
    pub frames: u64,
    /// Size of the picture sent (pixels), and of the ladder's rung (what the rate allows).
    pub send_px: u64,
    pub rung_px: u64,
    /// The cap from the peer's hint, its offer and my budget (`VideoSendCap::pixels`).
    pub cap_px: u64,
    /// The sender's rate estimate.
    pub rate_bps: u32,
    /// Overuse, starvation or a backlog drop since the last sample, or a queue being drained.
    pub congested: bool,
    /// The ladder stepped down for encode time since the last sample.
    pub cpu_step: bool,
    /// p75 encode time at the current size (us) and the current frame rate.
    pub encode_p75_us: Option<u64>,
    pub fps: u32,
    /// A hardware encoder: its wall time is pipeline latency, not CPU work, and it handles 720p.
    pub hw: bool,
}

/// My line, encoder and the peer's caps could carry a sharper picture than my HD-off capture
/// gives, this second.
pub(super) fn send_ok(s: &SendSample) -> bool {
    if s.hd_on || s.frames == 0 || s.send_px == 0 {
        return false;
    }
    // The capture is what holds the picture back: the ladder already allows a bigger one.
    let capture_bound = s.rung_px >= SHARPER_PIXELS && s.send_px < s.rung_px;
    let peer_takes_it = s.cap_px >= SHARPER_PIXELS;
    let line = s.rate_bps >= ROOM_BPS && !s.congested;
    let cpu = !s.cpu_step && (s.hw || fits(s.encode_p75_us, s.send_px, s.fps));
    capture_bound && peer_takes_it && line && cpu
}

/// One second of what I receive and draw.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct RecvSample {
    /// The hint I would send without the flag: shown (`max_pixels` > 0), not capped by my
    /// battery, heat or decoder (reason `DisplaySize`), and how big it asks for.
    pub wanted_px: u64,
    pub limited: bool,
    /// Frames decoded in the last second, and their size (pixels).
    pub frames: u64,
    pub recv_px: u64,
    /// p75 decode + render time (ms) over 2 s and how many samples; for a hardware decoder,
    /// whether it fell behind (it then has no meaningful time, see `adapt::hw_behind`).
    pub cost_ms: f32,
    pub cost_n: usize,
    pub hw: bool,
    pub behind: bool,
}

/// I draw the picture big enough for 540p, get less, and would decode and draw 540p in time.
pub(super) fn recv_ok(s: &RecvSample) -> bool {
    if s.limited || s.frames == 0 || s.recv_px == 0 {
        return false;
    }
    let big = s.wanted_px >= SHARPER_PIXELS && s.recv_px < SHARPER_PIXELS;
    let keeps_up = if s.hw {
        !s.behind
    } else {
        s.cost_n >= 10
            && (s.cost_ms as f64) * 1000.0 * (SHARPER_PIXELS as f64) / (s.recv_px as f64)
                <= budget_us(30) as f64
    };
    big && keeps_up
}

fn budget_us(fps: u32) -> u64 {
    1_000_000 * BUDGET_PCT / (100 * fps.max(1) as u64)
}

/// The p75 encode time scaled to a 540p picture fits the frame budget (time grows with pixels).
fn fits(p75_us: Option<u64>, px: u64, fps: u32) -> bool {
    p75_us.is_some_and(|p| px > 0 && p * SHARPER_PIXELS / px <= budget_us(fps))
}

/// Seconds in a row a rule has held; true from the [`ROOM_SECS`]th on, false at once when it
/// breaks.
#[derive(Debug, Default)]
pub(super) struct Streak(u32);

impl Streak {
    pub(super) fn observe(&mut self, ok: bool) -> bool {
        self.0 = if ok { self.0.saturating_add(1) } else { 0 };
        self.0 >= ROOM_SECS
    }
}

/// `VideoStats::hd_suggest`: "" (nothing), "line" (my line has room) or "peer" (my line has room
/// and the peer's side has room for a sharper picture too).
pub(super) fn suggestion(send_room: bool, peer_room: bool) -> &'static str {
    match (send_room, peer_room) {
        (false, _) => "",
        (true, false) => "line",
        (true, true) => "peer",
    }
}

/// What the device remembers about HD suggestions (each app stores the two numbers).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct HdSuggestMemory {
    /// Calls in which the person turned sharper video on with the chip.
    pub turned_on: u32,
    /// Calls in a row that showed a suggestion and ended without HD being turned on.
    pub ignored_in_a_row: u32,
}

/// After this many calls with HD turned on by hand the person knows the chip: no more
/// suggestions (Settings > Video has the lasting switch).
pub const HD_SUGGEST_MAX_TURNED_ON: u32 = 3;
/// After this many suggestions in a row went unanswered the person isn't interested: stop.
pub const HD_SUGGEST_MAX_IGNORED: u32 = 3;

/// Whether HD suggestions may still be shown on this device.
#[uniffi::export]
pub fn video_hd_suggest_allowed(memory: HdSuggestMemory) -> bool {
    memory.turned_on < HD_SUGGEST_MAX_TURNED_ON && memory.ignored_in_a_row < HD_SUGGEST_MAX_IGNORED
}

/// The memory after a call: `shown` = a suggestion was shown in it, `turned_on` = the person
/// turned sharper video on with the chip in it (suggested or not). Turning it on clears the
/// unanswered count; a call without a suggestion leaves it as it was.
#[uniffi::export]
pub fn video_hd_suggest_after_call(
    memory: HdSuggestMemory,
    shown: bool,
    turned_on: bool,
) -> HdSuggestMemory {
    if turned_on {
        HdSuggestMemory {
            turned_on: memory.turned_on.saturating_add(1),
            ignored_in_a_row: 0,
        }
    } else if shown {
        HdSuggestMemory {
            ignored_in_a_row: memory.ignored_in_a_row.saturating_add(1),
            ..memory
        }
    } else {
        memory
    }
}

/// Longest peer name in a suggestion before it is shortened (the line has one line of room).
const NAME_CHARS: usize = 12;

/// The suggestion to show now in the call's short status line, "" for none: from the stats
/// (`hd_suggest`), the app's own HD state for the call (a second guard: HD on never suggests)
/// and the device's memory. The app shows it at most once per call (one text for every app).
#[uniffi::export]
pub fn video_hd_suggestion(
    stats: &VideoStats,
    hd_on: bool,
    memory: HdSuggestMemory,
    peer_name: String,
) -> String {
    if hd_on || !video_hd_suggest_allowed(memory) {
        return String::new();
    }
    match stats.hd_suggest.as_str() {
        "line" => "Room for sharper video \u{b7} tap HD".into(),
        "peer" => {
            let name = peer_name.trim();
            let name = if name.is_empty() {
                "They".to_string()
            } else if name.chars().count() > NAME_CHARS {
                format!(
                    "{}\u{2026}",
                    name.chars()
                        .take(NAME_CHARS - 1)
                        .collect::<String>()
                        .trim_end()
                )
            } else {
                name.to_string()
            };
            format!("{name} could see you sharper \u{b7} tap HD")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
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
            .find(|(s, _)| st.observe(send_ok(s)))
            .map(|(_, i)| i)
    }

    #[test]
    fn video_call_hd_good_line_suggests_after_room_secs() {
        assert_eq!(first_room(std::iter::repeat_n(good(), 30)), Some(ROOM_SECS));
        // Right at the threshold counts; a hardware encoder needs no time.
        let edge = SendSample {
            rate_bps: ROOM_BPS,
            encode_p75_us: None,
            hw: true,
            ..good()
        };
        assert_eq!(first_room(std::iter::repeat_n(edge, 30)), Some(ROOM_SECS));
    }

    #[test]
    fn video_call_hd_weak_or_borderline_line_never() {
        // Below the 720p threshold for the whole call.
        let weak = SendSample {
            rate_bps: 1_100_000,
            ..good()
        };
        assert_eq!(first_room(std::iter::repeat_n(weak, 120)), None);
        // Borderline: above it, but a dip every 6 s.
        let calls = (0..120).map(|i| if i % 6 == 5 { weak } else { good() });
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
        // 9 good seconds, overuse, then it takes another full 10.
        let s = std::iter::repeat_n(good(), 9)
            .chain([over])
            .chain(std::iter::repeat_n(good(), 30));
        assert_eq!(first_room(s), Some(9 + 1 + ROOM_SECS));
        // Room already earned goes at once with an overuse, and is earned again from zero.
        let mut st = Streak::default();
        for _ in 0..ROOM_SECS {
            st.observe(send_ok(&good()));
        }
        assert!(st.observe(send_ok(&good())));
        assert!(!st.observe(send_ok(&over)));
        assert!(!st.observe(send_ok(&good())));
        // A CPU step down resets it the same way.
        let cpu = SendSample {
            cpu_step: true,
            ..good()
        };
        let s = std::iter::repeat_n(good(), 9)
            .chain([cpu])
            .chain(std::iter::repeat_n(good(), 30));
        assert_eq!(first_room(s), Some(9 + 1 + ROOM_SECS));
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
        // And the app's own guard: HD on, no line even if the stats said so.
        let st = VideoStats {
            hd_suggest: "line".into(),
            ..Default::default()
        };
        assert_eq!(
            video_hd_suggestion(&st, true, HdSuggestMemory::default(), "Ana".into()),
            ""
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

    #[test]
    fn video_call_hd_suggestion_kind_and_text() {
        assert_eq!(suggestion(false, true), "");
        assert_eq!(suggestion(true, false), "line");
        assert_eq!(suggestion(true, true), "peer");
        let m = HdSuggestMemory::default();
        let st = |k: &str| VideoStats {
            hd_suggest: k.into(),
            ..Default::default()
        };
        assert_eq!(video_hd_suggestion(&st(""), false, m, "Ana".into()), "");
        assert_eq!(
            video_hd_suggestion(&st("line"), false, m, "Ana".into()),
            "Room for sharper video \u{b7} tap HD"
        );
        assert_eq!(
            video_hd_suggestion(&st("peer"), false, m, "Ana".into()),
            "Ana could see you sharper \u{b7} tap HD"
        );
        assert_eq!(
            video_hd_suggestion(&st("peer"), false, m, "  ".into()),
            "They could see you sharper \u{b7} tap HD"
        );
        assert_eq!(
            video_hd_suggestion(&st("peer"), false, m, "Bartholomew Quince".into()),
            "Bartholomew\u{2026} could see you sharper \u{b7} tap HD"
        );
    }

    #[test]
    fn video_call_hd_memory_stops_after_uses_or_ignores() {
        let mut m = HdSuggestMemory::default();
        assert!(video_hd_suggest_allowed(m));
        // Ignored twice, then accepted: the count starts again.
        m = video_hd_suggest_after_call(m, true, false);
        m = video_hd_suggest_after_call(m, true, false);
        m = video_hd_suggest_after_call(m, false, false);
        assert_eq!(
            m,
            HdSuggestMemory {
                turned_on: 0,
                ignored_in_a_row: 2
            },
            "a call without a suggestion changes nothing"
        );
        m = video_hd_suggest_after_call(m, true, true);
        assert_eq!(
            m,
            HdSuggestMemory {
                turned_on: 1,
                ignored_in_a_row: 0
            }
        );
        // Three ignored in a row: stop.
        for _ in 0..3 {
            m = video_hd_suggest_after_call(m, true, false);
        }
        assert!(!video_hd_suggest_allowed(m));
        let st = VideoStats {
            hd_suggest: "line".into(),
            ..Default::default()
        };
        assert_eq!(video_hd_suggestion(&st, false, m, "Ana".into()), "");
        // Three calls with HD turned on by hand: stop too.
        let mut m = HdSuggestMemory::default();
        for _ in 0..3 {
            assert!(video_hd_suggest_allowed(m));
            m = video_hd_suggest_after_call(m, false, true);
        }
        assert!(!video_hd_suggest_allowed(m));
    }
}
