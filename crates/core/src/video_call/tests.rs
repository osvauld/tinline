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
