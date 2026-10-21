use super::*;

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
