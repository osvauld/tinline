use super::*;

#[test]
fn video_stats_keep_admission_receive_and_decode_distinct() {
    let mut st = VideoStats::default();
    st.bitrate_bps = 500_000;
    st.record_admitted(137);
    st.record_admitted(211);
    st.record_received(137);
    assert_eq!((st.admitted_payload_bytes, st.frames_sent), (348, 2));
    assert_eq!((st.received_payload_bytes, st.frames_received), (137, 1));
    assert_eq!(st.frames_decoded, 0);
    assert_eq!(st.bitrate_bps, 500_000, "target is not measured payload traffic");
}

#[test]
fn fit_keeps_aspect_and_orientation() {
    assert_eq!(fit(1280, 720, 640, 360), (640, 360));
    assert_eq!(fit(640, 480, 640, 360), (480, 360));
    assert_eq!(fit(720, 1280, 640, 360), (360, 640));
    assert_eq!(fit(320, 240, 1280, 720), (320, 240));
    assert_eq!(fit(1279, 721, 1280, 720), (1276, 720));
}

fn input(width: usize, height: usize, rotation: u32, src: Source) -> Input {
    Input { width, height, src, rotation, at: Instant::now() }
}

fn numbered(w: usize, h: usize) -> I420 {
    let mut pic = I420::new(w, h);
    for (i, b) in pic.data.iter_mut().enumerate() {
        *b = 16 + (i % 200) as u8;
    }
    pic
}

#[test]
fn rotations_compose() {
    let pic = numbered(6, 4);
    let turn = |p: &I420, deg| input(p.width, p.height, deg, Source::I420(p.clone())).prepare(1280, 720).unwrap();
    let r90 = turn(&pic, 90);
    assert_eq!((r90.width, r90.height), (4, 6));
    // Top-left after a clockwise turn is the old bottom-left.
    assert_eq!(r90.data[0], pic.data[3 * 6]);
    assert_eq!(turn(&r90, 270), pic);
    assert_eq!(turn(&turn(&pic, 180), 180), pic);
    assert_eq!(turn(&r90, 90), turn(&pic, 180));
    assert_eq!(turn(&pic, 45), pic);
}

#[test]
fn frames_fit_the_rung_upright() {
    let pic = numbered(1280, 720);
    let p = input(1280, 720, 0, Source::I420(pic.clone())).prepare(640, 360).unwrap();
    assert_eq!((p.width, p.height), (640, 360));
    let p = input(1280, 720, 90, Source::I420(pic)).prepare(640, 360).unwrap();
    assert_eq!((p.width, p.height), (360, 640));
    let p = input(640, 480, 270, Source::I420(numbered(640, 480))).prepare(640, 360).unwrap();
    assert_eq!((p.width, p.height), (360, 480));
}

/// A camera frame as Android's `YUV_420_888` with pixel stride 2 (NV21 memory: V then U
/// interleaved, `u` = `v[1..]`), padded rows, last row unpadded; full range.
fn android_planes(pic: &I420) -> (Vec<u8>, usize, Vec<u8>, Vec<u8>, usize) {
    let (w, h) = (pic.width, pic.height);
    let (cw, ch) = I420::chroma(w, h);
    let (sy, su, sv) = pic.planes();
    let ys = w + 16;
    let mut y = vec![0u8; (h - 1) * ys + w];
    for r in 0..h {
        y[r * ys..r * ys + w].copy_from_slice(&sy[r * w..(r + 1) * w]);
    }
    let uvs = 2 * cw + 8;
    let mut vu = vec![0u8; (ch - 1) * uvs + 2 * cw];
    for r in 0..ch {
        for c in 0..cw {
            vu[r * uvs + 2 * c] = sv[r * cw + c];
            vu[r * uvs + 2 * c + 1] = su[r * cw + c];
        }
    }
    let u = vu[1..].to_vec();
    vu.pop();
    (y, ys, u, vu, uvs)
}

#[test]
fn android_yuv_arrives_upright() {
    // Landscape sensor frame, phone held upright: rotation 90.
    let pic = numbered(640, 480);
    let (y, ys, u, v, uvs) = android_planes(&pic);
    assert!(plane_fits(y.len(), 640, 480, ys, 1) && plane_fits(u.len(), 320, 240, uvs, 2) && plane_fits(v.len(), 320, 240, uvs, 2));
    let src = Source::Yuv { y, y_stride: ys, u, v, uv_stride: uvs, uv_pixel_stride: 2, full_range: false };
    let got = input(640, 480, 90, src).prepare(1280, 720).unwrap();
    assert_eq!((got.width, got.height), (480, 640));
    let want = convert::transform(&pic, Rotation::R90, false);
    assert_eq!(got, want);
    // Scaled into a rung, still portrait and 3:4.
    let (y, ys, u, v, uvs) = android_planes(&pic);
    let src = Source::Yuv { y, y_stride: ys, u, v, uv_stride: uvs, uv_pixel_stride: 2, full_range: true };
    let got = input(640, 480, 90, src).prepare(640, 360).unwrap();
    assert_eq!((got.width, got.height), (360, 480));
    assert!(got.data.iter().all(|&b| (16..=240).contains(&b)), "limited range");
}

#[test]
fn nv21_as_one_plane() {
    let pic = numbered(640, 480);
    let (y, ys, u, v, uvs) = android_planes(&pic);
    let vu: Vec<u8> = [v[0]].into_iter().chain(u.iter().copied()).collect();
    let (cu, cv) = chroma_views(&[], &vu);
    assert!(plane_fits(cu.len(), 320, 240, uvs, 2) && plane_fits(cv.len(), 320, 240, uvs, 2));
    let src = Source::Yuv { y, y_stride: ys, u: vec![], v: vu, uv_stride: uvs, uv_pixel_stride: 2, full_range: false };
    let got = input(640, 480, 90, src).prepare(1280, 720).unwrap();
    assert_eq!(got, convert::transform(&pic, Rotation::R90, false));
}

/// `Bytes` crosses as `Vec<u8>` does, much faster. `--nocapture` prints the cost of one
/// 640x480 I420 frame each way (the Rust half of the FFI copy).
#[test]
fn bytes_cross_like_vec() {
    use uniffi::{Lift, Lower, RustBuffer};
    type T = crate::UniFfiTag;
    let data: Vec<u8> = (0..640 * 480 * 3 / 2).map(|i| i as u8).collect();
    let time = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        for _ in 0..20 {
            f();
        }
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    };
    let wire = <Vec<u8> as Lower<T>>::lower_into_rust_buffer(data.clone()).destroy_into_vec();
    assert_eq!(<Bytes as Lower<T>>::lower_into_rust_buffer(Bytes(data.clone())).destroy_into_vec(), wire);
    assert_eq!(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap().0, data);
    assert!(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire[..wire.len() - 1].to_vec())).is_err());
    let frame = VideoFrame { width: 640, height: 480, data: Bytes(data.clone()), seq: 1 };
    let mut r = &<VideoFrame as Lower<T>>::lower_into_rust_buffer(frame).destroy_into_vec()[..];
    assert_eq!(<VideoFrame as Lift<T>>::try_read(&mut r).unwrap().data.0, data);
    let lift_vec = time(&mut || drop(<Vec<u8> as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap()));
    let lift_bytes = time(&mut || drop(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap()));
    let lower_vec = time(&mut || <Vec<u8> as Lower<T>>::lower(data.clone()).destroy());
    let lower_bytes = time(&mut || <Bytes as Lower<T>>::lower(Bytes(data.clone())).destroy());
    println!("640x480 I420 ms (incl. a clone): lift Vec<u8> {lift_vec:.3} Bytes {lift_bytes:.3}, lower Vec<u8> {lower_vec:.3} Bytes {lower_bytes:.3}");
}

#[test]
fn chroma_spans_join_only_adjacent_views() {
    // NV21: V at 1000, U at 1001, each 99 bytes: one plane of 100 from V.
    assert_eq!(chroma_spans((1001, 99), (1000, 99), 2), ((0, 0), (1000, 100)));
    assert_eq!(chroma_spans((1000, 99), (1001, 99), 2), ((1000, 100), (0, 0)));
    // A longer first view keeps its own length.
    assert_eq!(chroma_spans((1001, 50), (1000, 99), 2), ((0, 0), (1000, 99)));
    // Planar, apart, empty or at the address limit: as given.
    assert_eq!(chroma_spans((1001, 99), (1000, 99), 1), ((1001, 99), (1000, 99)));
    assert_eq!(chroma_spans((2000, 99), (1000, 99), 2), ((2000, 99), (1000, 99)));
    assert_eq!(chroma_spans((1001, 0), (1000, 99), 2), ((1001, 0), (1000, 99)));
    assert_eq!(chroma_spans((0, 9), (usize::MAX, 1), 2), ((0, 9), (usize::MAX, 1)));
}

#[test]
fn short_planes_are_refused() {
    assert!(plane_fits(10, 4, 2, 6, 1));
    assert!(!plane_fits(9, 4, 2, 6, 1));
    assert!(!plane_fits(100, 4, 2, 3, 1));
    assert!(plane_fits(7, 2, 2, 4, 2) && !plane_fits(6, 2, 2, 4, 2));
}

#[test]
fn rgba_and_yuyv_inputs() {
    let rgba: Vec<u8> = (0..64 * 48).flat_map(|_| [200, 40, 40, 255]).collect();
    let got = input(64, 48, 180, Source::Rgba(rgba)).prepare(1280, 720).unwrap();
    assert_eq!((got.width, got.height), (64, 48));
    let yuyv: Vec<u8> = (0..64 * 48 / 2).flat_map(|_| [100, 90, 110, 160]).collect();
    let got = input(64, 48, 90, Source::Yuyv { data: yuyv, stride: 128 }).prepare(1280, 720).unwrap();
    assert_eq!((got.width, got.height), (48, 64));
    assert_eq!(got.data[0], 100);
}

#[test]
fn rgba_pull_converts() {
    let mut pic = I420::new(4, 2);
    pic.data.fill(128);
    pic.data[..8].fill(235);
    let f = to_rgba(VideoFrame { width: 4, height: 2, data: pic.data.into(), seq: 7 });
    assert_eq!((f.width, f.height, f.seq, f.data.len()), (4, 2, 7, 32));
    assert!(f.data.chunks(4).all(|p| p == [255, 255, 255, 255]));
}

#[test]
fn peer_cap_limits_the_ladder() {
    let l = LadderConfig::default();
    assert_eq!(max_rung(&l, 1280 * 720), 3);
    assert_eq!(max_rung(&l, 640 * 360), 1);
    assert_eq!(max_rung(&l, 1), 0);
}

#[test]
fn offer_has_av1_and_vp8_only_where_built_in() {
    let (decode, encode) = codecs::local_offer_codecs();
    assert_eq!(decode[0], VideoCodec::Av1);
    assert_eq!(decode.contains(&VideoCodec::Vp8), cfg!(feature = "vp8"));
    // Hosts have no hardware VP8 encoder: they always send AV1.
    assert_eq!(encode, vec![VideoCodec::Av1]);
    assert!(codecs::new_encoder(VideoCodec::Vp8, &EncoderConfig::default()).is_err());
    assert_eq!(codecs::new_decoder(VideoCodec::Vp8, MAX_PIXELS).is_ok(), cfg!(feature = "vp8"));
}

/// The receive path with the negotiated codec VP8: frames from the video crate's libvpx fixture
/// (see `video::vpx` tests for how it was made) come out as pictures and stats.
#[cfg(feature = "vp8")]
#[test]
fn vp8_frames_decode_on_the_receive_path() {
    const IVF: &[u8] = include_bytes!("../../../video/testdata/vp8_96x64.ivf");
    let mut payloads = Vec::new();
    let mut at = 32;
    while at + 12 <= IVF.len() {
        let n = u32::from_le_bytes(IVF[at..at + 4].try_into().unwrap()) as usize;
        payloads.push(IVF[at + 12..at + 12 + n].to_vec());
        at += 12 + n;
    }
    let pipe = Arc::new(Pipe::default());
    let (tx, rx) = mpsc::channel(payloads.len());
    for (i, payload) in payloads.into_iter().enumerate() {
        tx.try_send(Frame { group: 0, index: i as u32, capture_us: 0, payload }).unwrap();
    }
    drop(tx);
    decode_loop(pipe.clone(), rx, VideoCodec::Vp8);
    let st = pipe.stats.lock().clone();
    assert_eq!((st.frames_decoded, st.decode_errors), (8, 0));
    assert_eq!((st.recv_width, st.recv_height), (96, 64));
    let out = pipe.output.lock().take().unwrap();
    assert_eq!((out.width, out.height, out.seq), (96, 64, 8));
}

mod adapt_policy {
    use super::*;
    use std::time::Duration;
    use video::media::{Hint, HintReason};

    fn px(w: u32, h: u32) -> u32 {
        w * h
    }

    #[test]
    fn nothing_known_says_nothing() {
        let mut p = adapt::Policy::default();
        assert_eq!(p.update(Instant::now()), None);
    }

    #[test]
    fn drawn_size_rounds_up_to_a_rung() {
        let mut p = adapt::Policy::default();
        let t = Instant::now();
        for (w, h, want) in [(100, 60, px(320, 180)), (320, 180, px(320, 180)), (321, 180, px(640, 360)), (900, 500, px(960, 540)), (1920, 1080, u32::MAX)] {
            let mut p2 = adapt::Policy::default();
            p2.set_view(w, h, true);
            let h = p2.update(t).unwrap_or_else(|| Hint { max_pixels: u32::MAX, max_fps: 0, reason: HintReason::None });
            assert_eq!(h.max_pixels, want);
        }
        p.set_view(0, 0, false);
        assert_eq!(p.update(t).unwrap().max_pixels, 0);
    }

    #[test]
    fn shrinking_holds_a_second_growing_goes_at_once() {
        let mut p = adapt::Policy::default();
        let t = Instant::now();
        p.set_view(1280, 720, true);
        assert_eq!(p.update(t).unwrap().max_pixels, px(1280, 720));
        p.set_view(640, 360, true);
        assert_eq!(p.update(t + Duration::from_millis(100)), None);
        p.set_view(641, 361, true); // another rung: the wait restarts
        p.set_view(640, 360, true);
        assert_eq!(p.update(t + Duration::from_millis(600)), None);
        assert_eq!(p.update(t + Duration::from_millis(1700)).unwrap().max_pixels, px(640, 360));
        p.set_view(1280, 720, true);
        assert_eq!(p.update(t + Duration::from_millis(1800)).unwrap().max_pixels, px(1280, 720));
        // The same rung again is not news.
        p.set_view(1200, 700, true);
        assert_eq!(p.update(t + Duration::from_millis(1900)), None);
    }

    #[test]
    fn budget_caps_size_and_rate() {
        let mut p = adapt::Policy::default();
        let t = Instant::now();
        p.set_view(1280, 720, true);
        p.update(t);
        p.budget = VideoBudget { thermal: 2, power_save: false };
        let h = p.update(t).unwrap();
        assert_eq!((h.max_pixels, h.max_fps, h.reason), (px(960, 540), 15, HintReason::Thermal));
        p.budget = VideoBudget { thermal: 3, power_save: true };
        let h = p.update(t).unwrap();
        assert_eq!((h.max_pixels, h.max_fps, h.reason), (px(640, 360), 15, HintReason::Thermal));
        p.budget = VideoBudget { thermal: 0, power_save: true };
        let h = p.update(t).unwrap();
        assert_eq!((h.max_pixels, h.max_fps, h.reason), (px(960, 540), 24, HintReason::PowerSave));
        // Our own ladder follows the budget.
        assert_eq!(p.own_caps(), (2, 24));
        p.budget = VideoBudget::default();
        assert_eq!(p.own_caps(), (3, u32::MAX));
    }

    #[test]
    fn decode_cost_steers_the_rate_tier() {
        let mut p = adapt::Policy::default();
        let t = Instant::now();
        p.set_view(1280, 720, true);
        p.update(t);
        // 25 ms is 75% of a 33 ms interval: down to 24, then (60 ms is 144% of 41) to 15.
        assert!(p.observe_decode(t, 25.0, 30));
        assert_eq!(p.update(t).unwrap().max_fps, 24);
        assert!(p.observe_decode(t, 30.0, 30));
        let h = p.update(t).unwrap();
        assert_eq!((h.max_fps, h.reason), (15, HintReason::DecodeSlow));
        // Too few samples say nothing; cheap decoding must hold 10 s before a tier up.
        assert!(!p.observe_decode(t, 100.0, 3));
        assert!(!p.observe_decode(t, 5.0, 30));
        assert!(!p.observe_decode(t + Duration::from_secs(9), 5.0, 30));
        assert!(p.observe_decode(t + Duration::from_secs(10), 5.0, 30));
        assert_eq!(p.update(t).unwrap().max_fps, 24);
    }

    #[test]
    fn send_cap_names_what_binds() {
        let mut p = adapt::Policy::default();
        let (cap, src) = p.send_cap(None);
        assert_eq!((cap.pixels, cap.fps, cap.reason.as_str(), src), (px(1280, 720), 30, "none", ""));
        let hint = Hint { max_pixels: px(320, 180), max_fps: 15, reason: HintReason::Thermal };
        let (cap, src) = p.send_cap(Some(hint));
        assert_eq!((cap.pixels, cap.fps, cap.reason.as_str(), src), (px(320, 180), 15, "thermal", "hint"));
        let hidden = Hint { max_pixels: 0, max_fps: 0, reason: HintReason::DisplaySize };
        assert_eq!(p.send_cap(Some(hidden)).0.fps, 15);
        p.budget = VideoBudget { thermal: 0, power_save: true };
        let (cap, src) = p.send_cap(None);
        assert_eq!((cap.pixels, cap.fps, cap.reason.as_str(), src), (px(960, 540), 24, "battery", "budget"));
    }
}
