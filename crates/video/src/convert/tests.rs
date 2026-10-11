use super::*;

fn pic(w: usize, h: usize, y: &[u8], u: &[u8], v: &[u8]) -> I420 {
    I420 { width: w, height: h, data: [y, u, v].concat() }
}

/// I420 where every sample encodes its position: Y = 10*y + x, U = 100 + 10*cy + cx, V = U + 50.
fn coords(w: usize, h: usize) -> I420 {
    let (cw, ch) = I420::chroma(w, h);
    let y: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (10 * y + x) as u8)).collect();
    let u: Vec<u8> = (0..ch).flat_map(|y| (0..cw).map(move |x| (100 + 10 * y + x) as u8)).collect();
    let v: Vec<u8> = u.iter().map(|s| s + 50).collect();
    pic(w, h, &y, &u, &v)
}

/// Lays `p` out as Android YUV_420_888 with padding, pixel stride `ps` (2 = NV12 view), and
/// planes that end right after their last sample.
fn android(p: &I420, y_pad: usize, uv_pad: usize, ps: usize) -> (Vec<u8>, usize, Vec<u8>, Vec<u8>, usize) {
    let (w, h) = (p.width, p.height);
    let (cw, ch) = I420::chroma(w, h);
    let (sy, su, sv) = p.planes();
    let ys = w + y_pad;
    let mut y = vec![0xEE; (h - 1) * ys + w];
    for r in 0..h {
        y[r * ys..r * ys + w].copy_from_slice(&sy[r * w..(r + 1) * w]);
    }
    let cs = cw * ps + uv_pad;
    let mut u = vec![0xEE; (ch - 1) * cs + (cw - 1) * ps + 1];
    let mut v = u.clone();
    for r in 0..ch {
        for c in 0..cw {
            u[r * cs + c * ps] = su[r * cw + c];
            v[r * cs + c * ps] = sv[r * cw + c];
        }
    }
    (y, ys, u, v, cs)
}

fn view<'a>(a: &'a (Vec<u8>, usize, Vec<u8>, Vec<u8>, usize), ps: usize, w: usize, h: usize) -> Yuv420<'a> {
    Yuv420 { y: &a.0, y_stride: a.1, u: &a.2, v: &a.3, uv_stride: a.4, uv_pixel_stride: ps, width: w, height: h }
}

#[test]
fn convert_strided_planes_to_i420() {
    for (w, h) in [(4, 4), (5, 3), (6, 2), (1, 1)] {
        let p = coords(w, h);
        for ps in [1, 2] {
            for (yp, cp) in [(0, 0), (3, 1), (16, 7)] {
                let a = android(&p, yp, cp, ps);
                assert_eq!(yuv420_to_i420(&view(&a, ps, w, h), None).unwrap(), p, "{w}x{h} ps{ps} pad{yp},{cp}");
            }
        }
    }
}

#[test]
fn convert_nv12_and_nv21_interleaved() {
    // 2x2 frame, one chroma sample: NV12 = U V, NV21 = V U.
    let y = [1, 2, 3, 4];
    let i = yuv420_to_i420(&Yuv420::nv12(&y, 2, &[7, 9], 2, 2, 2, false), None).unwrap();
    assert_eq!(i.data, vec![1, 2, 3, 4, 7, 9]);
    let i = yuv420_to_i420(&Yuv420::nv12(&y, 2, &[7, 9], 2, 2, 2, true), None).unwrap();
    assert_eq!(i.data, vec![1, 2, 3, 4, 9, 7]);
}

#[test]
fn convert_rejects_short_planes() {
    let p = coords(4, 4);
    let mut a = android(&p, 2, 1, 2);
    a.2.pop();
    assert!(matches!(yuv420_to_i420(&view(&a, 2, 4, 4), None), Err(Error::BadFrame(..))));
    let a = android(&p, 0, 0, 1);
    let crop = Rect { x: 2, y: 0, w: 3, h: 4 };
    assert!(yuv420_to_i420(&view(&a, 1, 4, 4), Some(crop)).is_err());
}

#[test]
fn convert_crop_aligns_origin_to_even() {
    let p = coords(6, 4);
    let a = android(&p, 2, 3, 2);
    let got = yuv420_to_i420(&view(&a, 2, 6, 4), Some(Rect { x: 3, y: 1, w: 3, h: 2 })).unwrap();
    // Origin (3,1) -> (2,0); chroma from (1,0), 2x1 samples.
    assert_eq!(got, pic(3, 2, &[2, 3, 4, 12, 13, 14], &[101, 102], &[151, 152]));
}

#[test]
fn convert_rotations_and_mirror() {
    // Y plane 3x2:  0 1 2 / 10 11 12. Chroma 2x1: 100 101.
    let p = coords(3, 2);
    let r = |rot, m| transform(&p, rot, m);
    assert_eq!(r(Rotation::R0, false), p);
    assert_eq!(r(Rotation::R0, true), pic(3, 2, &[2, 1, 0, 12, 11, 10], &[101, 100], &[151, 150]));
    assert_eq!(r(Rotation::R90, false), pic(2, 3, &[10, 0, 11, 1, 12, 2], &[100, 101], &[150, 151]));
    assert_eq!(r(Rotation::R90, true), pic(2, 3, &[0, 10, 1, 11, 2, 12], &[100, 101], &[150, 151]));
    assert_eq!(r(Rotation::R180, false), pic(3, 2, &[12, 11, 10, 2, 1, 0], &[101, 100], &[151, 150]));
    assert_eq!(r(Rotation::R180, true), pic(3, 2, &[10, 11, 12, 0, 1, 2], &[100, 101], &[150, 151]));
    assert_eq!(r(Rotation::R270, false), pic(2, 3, &[2, 12, 1, 11, 0, 10], &[101, 100], &[151, 150]));
    assert_eq!(r(Rotation::R270, true), pic(2, 3, &[12, 2, 11, 1, 10, 0], &[101, 100], &[151, 150]));
}

#[test]
fn convert_rotation_identities_on_large_odd_frame() {
    // Bigger than one 32x32 tile and odd-sized: exercises tiling and odd chroma.
    let (w, h) = (77, 45);
    let p = I420 { width: w, height: h, data: (0..I420::len(w, h)).map(|i| (i * 7 % 251) as u8).collect() };
    let r90 = transform(&p, Rotation::R90, false);
    assert_eq!((r90.width, r90.height), (45, 77));
    assert_eq!(transform(&r90, Rotation::R270, false), p);
    assert_eq!(transform(&transform(&r90, Rotation::R90, false), Rotation::R180, false), p);
    assert_eq!(transform(&transform(&p, Rotation::R0, true), Rotation::R0, true), p);
    // Mirror after 180 = vertical flip.
    let (sy, ..) = p.planes();
    assert_eq!(&transform(&p, Rotation::R180, true).data[..w], &sy[(h - 1) * w..]);
}

#[test]
fn convert_scale_paths() {
    // Exact 2:1 box with rounding.
    let p = pic(4, 2, &[0, 1, 10, 10, 2, 2, 20, 21], &[10, 20], &[30, 40]);
    assert_eq!(scale(&p, 2, 1), pic(2, 1, &[1, 15], &[15], &[35]));
    // General 3:1 horizontally, 1:1 vertically.
    let p = pic(6, 1, &[0, 3, 6, 9, 9, 10], &[1, 2, 6], &[0, 0, 0]);
    assert_eq!(scale(&p, 2, 1), pic(2, 1, &[3, 9], &[3], &[0]));
    // Upscale is nearest neighbour.
    let p = pic(2, 2, &[1, 2, 3, 4], &[5], &[6]);
    assert_eq!(scale(&p, 4, 2), pic(4, 2, &[1, 1, 2, 2, 3, 3, 4, 4], &[5, 5], &[6, 6]));
    // Constant frames stay constant at any ratio.
    let c = I420 { width: 1280, height: 720, data: vec![200; I420::len(1280, 720)] };
    for (w, h) in [(640, 360), (960, 540), (426, 240), (7, 5)] {
        assert!(scale(&c, w, h).data.iter().all(|&s| s == 200));
    }
}

#[test]
fn convert_fit_center_crops_to_aspect() {
    // 8x4 -> 2x2: center 4x4 (x 2..6), then 2:1.
    let mut p = I420::new(8, 4);
    {
        let (y, ..) = p.planes_mut();
        for (i, s) in y.iter_mut().enumerate() {
            *s = if (2..6).contains(&(i % 8)) { 100 } else { 0 };
        }
    }
    let f = fit(&p, 2, 2);
    assert_eq!(&f.data[..4], &[100; 4]);
}

#[test]
fn convert_prepare_camera_frame() {
    // 8x6 YUV_420_888 (pixel stride 2, padded), rotate 90 -> 2x4 output: pre-rotation target
    // 4x2 = aspect 2:1, center crop of 8x6 is 8x4 at y=1 -> even y=0, then 2:1 box.
    let mut p = I420::new(8, 6);
    {
        let (y, u, v) = p.planes_mut();
        for (i, s) in y.iter_mut().enumerate() {
            *s = (i / 8 * 10 + i % 8) as u8;
        }
        u.fill(90);
        v.fill(160);
    }
    let a = android(&p, 8, 5, 2);
    let cfg = Prepare { rotate: Rotation::R90, mirror: false, width: 2, height: 4, range: Range::Limited };
    let out = prepare(&view(&a, 2, 8, 6), None, &cfg).unwrap();
    // Pre-rotation 4x2 = 2:1 box of rows 0..4, e.g. (0 + 1 + 10 + 11 + 2) / 4 = 6.
    let pre = pic(4, 2, &[6, 8, 10, 12, 26, 28, 30, 32], &[90, 90], &[160, 160]);
    assert_eq!(out, transform(&pre, Rotation::R90, false));
    assert_eq!((out.width, out.height), (2, 4));
    // Odd output sizes round down to even; full range is compressed.
    let cfg = Prepare { rotate: Rotation::R0, mirror: true, width: 9, height: 5, range: Range::Full };
    let out = prepare(&view(&a, 2, 8, 6), None, &cfg).unwrap();
    assert_eq!((out.width, out.height), (8, 4));
}

#[test]
fn convert_full_to_limited_endpoints() {
    let mut p = pic(2, 2, &[0, 255, 128, 1], &[0], &[255]);
    full_to_limited(&mut p);
    assert_eq!(p.data, vec![16, 235, 126, 17, 16, 240]);
    let mut p = pic(2, 2, &[0; 4], &[128], &[128]);
    full_to_limited(&mut p);
    assert_eq!(&p.data[4..], &[128, 128]);
}

#[test]
fn convert_yuyv() {
    // 2x2 (stride 6, padded) and odd height 2x3.
    let src = [10, 100, 20, 200, 0, 0, 30, 111, 40, 211, 0, 0];
    let p = yuyv_to_i420(&src, 6, 2, 2).unwrap();
    assert_eq!(p.data, vec![10, 20, 30, 40, 106, 206]);
    let src = [1, 50, 2, 60, 3, 52, 4, 62, 5, 70, 6, 80];
    let p = yuyv_to_i420(&src, 4, 2, 3).unwrap();
    assert_eq!(p.data, vec![1, 2, 3, 4, 5, 6, 51, 70, 61, 80]);
    assert!(yuyv_to_i420(&src[..11], 4, 2, 3).is_err());
}

#[test]
fn convert_rgb_exact_values() {
    // Known BT.601 limited values: black, white, red, green, blue.
    let cases = [
        ([0, 0, 0], [16, 128, 128]),
        ([255, 255, 255], [235, 128, 128]),
        ([255, 0, 0], [82, 90, 240]),
        ([0, 255, 0], [144, 54, 34]),
        ([0, 0, 255], [41, 240, 110]),
    ];
    for (rgb, yuv) in cases {
        let px: Vec<u8> = rgb.iter().chain(&[255]).copied().collect();
        let rgba: Vec<u8> = px.repeat(4);
        let p = rgb_to_i420(&rgba, 8, 2, 2, RgbLayout::Rgba).unwrap();
        assert_eq!(p.data, vec![yuv[0], yuv[0], yuv[0], yuv[0], yuv[1], yuv[2]], "{rgb:?}");
        let bgr: Vec<u8> = [rgb[2], rgb[1], rgb[0]].repeat(4);
        assert_eq!(rgb_to_i420(&bgr, 6, 2, 2, RgbLayout::Bgr).unwrap(), p);
        let bgra: Vec<u8> = [rgb[2], rgb[1], rgb[0], 0].repeat(4);
        assert_eq!(rgb_to_i420(&bgra, 8, 2, 2, RgbLayout::Bgra).unwrap(), p);
        let rgb3: Vec<u8> = rgb.repeat(4);
        assert_eq!(rgb_to_i420(&rgb3, 6, 2, 2, RgbLayout::Rgb).unwrap(), p);
        let back = to_rgba(&p);
        for (b, a) in back.chunks(4).zip(rgba.chunks(4)) {
            for c in 0..3 {
                assert!((b[c] as i32 - a[c] as i32).abs() <= 2, "{rgb:?} -> {back:?}");
            }
        }
    }
}

#[test]
fn convert_rgb_chroma_averages_with_odd_edges() {
    // 3x1 RGB with stride padding: chroma 2x1; second sample from the lone last pixel.
    let src = [255, 0, 0, 255, 0, 0, 0, 0, 255, 9, 9];
    let p = rgb_to_i420(&src, 11, 3, 1, RgbLayout::Rgb).unwrap();
    assert_eq!(p.data, vec![82, 82, 41, 90, 240, 240, 110]);
}

#[test]
fn convert_i420_to_rgba_exact_and_odd() {
    // 3x1: black, white with neutral chroma; the odd last pixel has its own (red) sample.
    let p = pic(3, 1, &[16, 235, 82], &[128, 90], &[128, 240]);
    assert_eq!(to_rgba(&p), vec![0, 0, 0, 255, 255, 255, 255, 255, 255, 1, 0, 255]);
    let p = pic(3, 3, &[16; 9], &[128; 4], &[128; 4]);
    assert!(to_rgba(&p).chunks(4).all(|px| px == [0, 0, 0, 255]));
    assert!(i420_to_rgba(&p, &mut [0; 35]).is_err());
}

#[test]
fn convert_rgb_round_trip() {
    // Random colours, constant per 2x2 block (chroma subsampling is lossy otherwise).
    let (w, h) = (34, 18);
    let mut s = 12345u32;
    let mut rnd = || {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (s >> 16) as u8
    };
    let blocks: Vec<[u8; 3]> = (0..(w / 2) * (h / 2)).map(|_| [rnd(), rnd(), rnd()]).collect();
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let c = blocks[(y / 2) * (w / 2) + x / 2];
            rgba[(y * w + x) * 4..][..4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    let back = to_rgba(&rgb_to_i420(&rgba, w * 4, w, h, RgbLayout::Rgba).unwrap());
    let max = rgba.iter().zip(&back).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
    assert!(max <= 3, "max error {max}");
}
