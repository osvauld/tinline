//! Video in 1:1 calls: two real nodes in one process over iroh (needs network for relays and
//! discovery, like `calls.rs`). A synthetic camera (`video::Pattern`) is pushed on one side and
//! decoded frames are pulled on the other.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use p2pcore::{CallInfo, CallState, Node, NodeEvents, NodeStatus, Pulled, VideoBudget, VideoEvents, VideoInfo};

mod support;
use support::{PASS, TempDir, online};

#[derive(Debug, Clone)]
enum Ev {
    Incoming(CallInfo),
    State(String, CallState),
    Video(VideoInfo),
}

struct Tap(Mutex<mpsc::Sender<Ev>>);

impl NodeEvents for Tap {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, c: CallInfo) {
        let _ = self.0.lock().unwrap().send(Ev::Incoming(c));
    }
    fn on_call_state(&self, id: String, s: CallState) {
        let _ = self.0.lock().unwrap().send(Ev::State(id, s));
    }
    fn on_log(&self, _: String) {}
}

impl VideoEvents for Tap {
    fn on_video_state(&self, info: VideoInfo) {
        let _ = self.0.lock().unwrap().send(Ev::Video(info));
    }
}

struct Peer {
    node: Arc<Node>,
    rx: mpsc::Receiver<Ev>,
    did: String,
    _dir: TempDir,
}

fn peer(name: &str) -> Peer {
    let dir = TempDir::new("test", name);
    let (tx, rx) = mpsc::channel();
    let tap = Arc::new(Tap(Mutex::new(tx)));
    let node = Node::new(dir.to_string_lossy().into(), tap.clone()).unwrap();
    node.auto_accept_contacts_for_test(true);
    node.set_video_events(tap);
    node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    Peer { node, rx, did, _dir: dir }
}

fn pair(tag: &str) -> (Peer, Peer) {
    let a = peer(&format!("{tag}-a"));
    let b = peer(&format!("{tag}-b"));
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    (a, b)
}

fn expect<T>(p: &Peer, secs: u64, what: &str, mut f: impl FnMut(&Ev) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p.rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(ev) => {
                if let Some(t) = f(&ev) {
                    return t;
                }
            }
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

fn video_ev(p: &Peer, what: &str, f: impl Fn(&VideoInfo) -> bool) -> VideoInfo {
    expect(p, 15, what, |e| match e {
        Ev::Video(v) if f(v) => Some(v.clone()),
        _ => None,
    })
}

/// b calls a, a answers; returns the call id once both are active.
fn connect(a: &Peer, b: &Peer) -> String {
    let call = b.node.call(a.did.clone()).unwrap();
    let inc = expect(a, 40, "incoming", |e| match e {
        Ev::Incoming(c) => Some(c.call_id.clone()),
        _ => None,
    });
    assert_eq!(inc, call.call_id);
    a.node.answer(inc).unwrap();
    for p in [a, b] {
        expect(p, 20, "active", |e| matches!(e, Ev::State(_, CallState::Active)).then_some(()));
    }
    call.call_id
}

/// Pushes 640x480 pattern frames at 30 fps until dropped: packed I420, or (`nv21`) as an Android
/// camera's padded Y plane and one interleaved VU plane through `push_video_planes` (what the
/// Android JNI entry point calls).
struct Camera {
    run: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<u32>>,
}

impl Camera {
    fn start(node: Arc<Node>, nv21: bool) -> Self {
        let run = Arc::new(AtomicBool::new(true));
        let r = run.clone();
        let thread = std::thread::spawn(move || {
            let mut pat = video::Pattern::new(640, 480);
            let mut accepted = 0;
            while r.load(Ordering::SeqCst) {
                let f = pat.next_frame();
                accepted += if nv21 {
                    let (y, vu, stride) = to_nv21(&f.data, 640, 480);
                    node.push_video_planes(640, 480, 0, &y, stride, &[], &vu, stride, 2, false)
                } else {
                    node.push_video_frame(640, 480, 0, f.data)
                } as u32;
                std::thread::sleep(Duration::from_millis(33));
            }
            accepted
        });
        Self { run, thread: Some(thread) }
    }

    fn stop(mut self) -> u32 {
        self.run.store(false, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap()
    }
}

/// I420 `w` x `h` as Android lays out NV21: rows padded to `stride`, the last row of each plane
/// unpadded, V and U interleaved in one plane.
fn to_nv21(i420: &[u8], w: usize, h: usize) -> (Vec<u8>, Vec<u8>, usize) {
    let stride = w + 32;
    let (cw, ch) = (w / 2, h / 2);
    let (sy, chroma) = i420.split_at(w * h);
    let (su, sv) = chroma.split_at(cw * ch);
    let mut y = vec![0; (h - 1) * stride + w];
    for r in 0..h {
        y[r * stride..r * stride + w].copy_from_slice(&sy[r * w..(r + 1) * w]);
    }
    let mut vu = vec![0; (ch - 1) * stride + 2 * cw];
    for r in 0..ch {
        for c in 0..cw {
            vu[r * stride + 2 * c] = sv[r * cw + c];
            vu[r * stride + 2 * c + 1] = su[r * cw + c];
        }
    }
    (y, vu, stride)
}

/// `watch` through `pull_video_frame_rgba_into`, growing the buffer on `TooSmall` as Android's
/// renderer does.
fn watch_rgba(p: &Peer, d: Duration) -> (u32, Option<(u32, u32)>) {
    let end = Instant::now() + d;
    let (mut n, mut size, mut last_seq, mut buf) = (0, None, 0, Vec::new());
    while Instant::now() < end {
        match p.node.pull_video_frame_rgba_into(&mut buf) {
            Pulled::None => {}
            Pulled::TooSmall { width, height } => buf = vec![0; width as usize * height as usize * 4],
            Pulled::Frame { width, height, seq } => {
                assert!(seq > last_seq && buf.len() >= (width * height * 4) as usize);
                assert!(buf[..(width * height * 4) as usize].chunks(4).all(|px| px[3] == 255));
                last_seq = seq;
                n += 1;
                size = Some((width, height));
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    (n, size)
}

/// Pulls frames for `d`, like a renderer polling at ~100 Hz: (count, last size).
fn watch(p: &Peer, d: Duration) -> (u32, Option<(u32, u32)>) {
    let end = Instant::now() + d;
    let (mut n, mut size, mut last_seq) = (0, None, 0);
    while Instant::now() < end {
        if let Some(f) = p.node.pull_video_frame() {
            assert_eq!(f.data.len(), (f.width * f.height + 2 * f.width.div_ceil(2) * f.height.div_ceil(2)) as usize);
            assert!(f.seq > last_seq);
            last_seq = f.seq;
            n += 1;
            size = Some((f.width, f.height));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    (n, size)
}

#[test]
fn video_both_ways() {
    let (a, b) = pair("vboth");
    let id = connect(&a, &b);
    for p in [&a, &b] {
        let v = video_ev(p, "negotiated", |v| v.negotiated);
        assert_eq!(v.call_id, id);
        p.node.set_camera(true).unwrap();
    }
    for p in [&a, &b] {
        video_ev(p, "remote camera on", |v| v.remote_camera && v.local_camera);
    }
    // a's camera as Android's NV21 planes; a's renderer pulls RGBA into its own buffer.
    let cams = [Camera::start(a.node.clone(), true), Camera::start(b.node.clone(), false)];
    // Let the first keyframes through, then count.
    std::thread::sleep(Duration::from_millis(500));
    for p in [&a, &b] {
        let (n, size) = if std::ptr::eq(p, &a) { watch_rgba(p, Duration::from_secs(3)) } else { watch(p, Duration::from_secs(3)) };
        let (w, h) = size.expect("decoded frames");
        assert!(n >= 45, "{n} frames in 3 s");
        // 4:3 kept: 480x360 at the 360p rung, 640x480 (never upscaled) above it.
        assert_eq!(w * 3, h * 4, "{w}x{h}");
        let s = p.node.video_stats().unwrap();
        assert_eq!(s.decode_errors, 0, "{s:?}");
        assert!(s.frames_sent >= 60 && s.keyframes_sent >= 1, "{s:?}");
        println!("{s:?}");
    }
    for c in cams {
        assert!(c.stop() > 60);
    }
    b.node.hangup(id.clone()).unwrap();
    for p in [&a, &b] {
        expect(p, 10, "ended", |e| matches!(e, Ev::State(i, CallState::Ended { .. }) if *i == id).then_some(()));
    }
    // Gone with the call.
    assert!(a.node.pull_video_frame().is_none() && a.node.video_info().is_none());
    assert_eq!(a.node.pull_video_frame_rgba_into(&mut [0; 16]), Pulled::None);
    // Direct frame calls find a node by a handle: one per node, stable.
    let h = a.node.video_frame_handle();
    assert!(h != 0 && h == a.node.video_frame_handle() && h != b.node.video_frame_handle());
    assert!(!a.node.push_video_frame(640, 480, 0, vec![0; 640 * 480 * 3 / 2]));
}

#[test]
fn camera_off_and_on_mid_call() {
    let (a, b) = pair("vcam");
    // Camera on before the call is even answered: applies once video runs.
    let call = b.node.call(a.did.clone()).unwrap();
    b.node.set_camera(true).unwrap();
    let inc = expect(&a, 40, "incoming", |e| match e {
        Ev::Incoming(c) => Some(c.call_id.clone()),
        _ => None,
    });
    a.node.answer(inc).unwrap();
    video_ev(&a, "b's camera on", |v| v.negotiated && v.remote_camera && !v.local_camera);
    let cam = Camera::start(b.node.clone(), false);
    std::thread::sleep(Duration::from_millis(500));
    let (n, _) = watch(&a, Duration::from_secs(2));
    assert!(n >= 30, "{n} frames before off");

    b.node.set_camera(false).unwrap();
    video_ev(&a, "b's camera off", |v| !v.remote_camera);
    std::thread::sleep(Duration::from_millis(300));
    while a.node.pull_video_frame().is_some() {}
    let (n, _) = watch(&a, Duration::from_secs(1));
    assert_eq!(n, 0, "frames while the camera is off");
    let before = a.node.video_stats().unwrap();

    b.node.set_camera(true).unwrap();
    video_ev(&a, "b's camera on again", |v| v.remote_camera);
    std::thread::sleep(Duration::from_millis(300));
    let (n, size) = watch(&a, Duration::from_secs(2));
    assert!(n >= 30, "{n} frames after on");
    assert!(size.is_some());
    let after = a.node.video_stats().unwrap();
    assert_eq!(after.decode_errors, 0, "{after:?}");
    // Resuming starts a new group.
    let sent = b.node.video_stats().unwrap();
    assert!(sent.keyframes_sent >= 2, "{sent:?}");
    assert!(after.frames_decoded > before.frames_decoded);
    cam.stop();
    b.node.hangup(call.call_id).unwrap();
}

#[test]
fn video_refused_call_stays_audio_only() {
    let (a, b) = pair("vrefuse");
    a.node.set_video_enabled(false);
    let id = connect(&a, &b);
    let v = video_ev(&b, "video decided", |_| true);
    assert!(!v.negotiated);
    assert!(!a.node.video_info().unwrap().negotiated);
    assert!(b.node.set_camera(true).is_err());
    assert!(!b.node.push_video_frame(640, 480, 0, vec![0; 640 * 480 * 3 / 2]));
    // Audio still flows.
    for _ in 0..50 {
        b.node.push_mic(vec![0i16; 960]);
        std::thread::sleep(Duration::from_millis(20));
    }
    let s = a.node.call_stats().unwrap();
    assert!(s.received > 10 && !s.reconnecting, "{s:?}");
    assert_eq!(a.node.call_stats().unwrap().state, CallState::Active);
    b.node.hangup(id.clone()).unwrap();
    let reason = expect(&a, 10, "ended", |e| match e {
        Ev::State(i, CallState::Ended { reason }) if *i == id => Some(reason.clone()),
        _ => None,
    });
    assert_eq!(reason, "hangup_remote");
}

/// Polls `p`'s stats until `f` holds (up to `secs`); returns the stats that satisfied it.
fn stats_when(p: &Peer, secs: u64, what: &str, f: impl Fn(&p2pcore::VideoStats) -> bool) -> p2pcore::VideoStats {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let s = p.node.video_stats().unwrap();
        if f(&s) {
            return s;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}: {s:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Receiver hints (docs/design/video-adaptation.md, test 7): b draws a's picture small, then
/// reports a thermal state; a's sender is capped within a second and sends at most 16 fps
/// within two more.
#[test]
fn receiver_hints_cap_the_sender() {
    let (a, b) = pair("vhint");
    connect(&a, &b);
    for p in [&a, &b] {
        video_ev(p, "negotiated", |v| v.negotiated);
    }
    a.node.set_camera(true).unwrap();
    video_ev(&b, "remote camera on", |v| v.remote_camera);
    let cam = Camera::start(a.node.clone(), false);
    // Uncapped first: 30 fps, the top rung.
    let s = stats_when(&a, 10, "sending", |s| s.send_fps >= 20);
    assert_eq!((s.send_cap.pixels, s.send_cap.fps, s.send_cap.reason.as_str()), (1280 * 720, 30, "none"), "{s:?}");

    b.node.set_video_view(320, 180, true);
    let s = stats_when(&a, 1, "display cap", |s| s.send_cap.pixels == 320 * 180);
    assert_eq!(s.send_cap.reason, "display", "{s:?}");

    b.node.set_video_budget(VideoBudget { thermal: 2, power_save: false });
    let s = stats_when(&a, 1, "thermal cap", |s| s.send_cap.fps == 15);
    assert_eq!((s.send_cap.pixels, s.send_cap.reason.as_str()), (320 * 180, "thermal"), "{s:?}");
    let s = stats_when(&a, 2, "<= 16 fps", |s| s.send_fps > 0 && s.send_fps <= 16);
    assert!(s.send_width <= 320 && s.send_height <= 180, "{s:?}");
    println!("{s:?}");

    // Lifting the budget and showing the picture big lifts the cap again.
    b.node.set_video_budget(VideoBudget::default());
    b.node.set_video_view(1280, 720, true);
    stats_when(&a, 1, "cap lifted", |s| s.send_cap.pixels == 1280 * 720 && s.send_cap.fps == 30);
    cam.stop();
}
