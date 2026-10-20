use super::*;

/// A 4x2 YUYV picture: Y ramps, U/V differ per row so the row-pair average shows.
fn yuyv(stride: usize) -> Vec<u8> {
    let mut b = vec![0xEE; stride * 2];
    for (y, row) in [
        [10, 100, 20, 200, 30, 102, 40, 202],
        [50, 110, 60, 210, 70, 112, 80, 212],
    ]
    .iter()
    .enumerate()
    {
        b[y * stride..y * stride + 8].copy_from_slice(row);
    }
    b
}

#[test]
fn yuyv_tight_and_padded() {
    for stride in [8, 16] {
        let p = to_i420(
            Pix::Yuyv,
            &yuyv(stride),
            if stride == 8 { 0 } else { stride },
            4,
            2,
            Range::Limited,
        )
        .unwrap();
        assert_eq!((p.width, p.height), (4, 2));
        let (y, u, v) = p.planes();
        assert_eq!(y, [10, 20, 30, 40, 50, 60, 70, 80]);
        assert_eq!(u, [105, 107]);
        assert_eq!(v, [205, 207]);
    }
}

#[test]
fn nv12_padded() {
    let (w, h, stride) = (4, 2, 6);
    let mut b = vec![0xEE; stride * h + stride];
    b[..4].copy_from_slice(&[1, 2, 3, 4]);
    b[stride..stride + 4].copy_from_slice(&[5, 6, 7, 8]);
    b[stride * h..stride * h + 4].copy_from_slice(&[100, 200, 101, 201]);
    let p = to_i420(Pix::Nv12, &b, stride, w, h, Range::Limited).unwrap();
    assert_eq!(
        p.planes(),
        (
            &[1, 2, 3, 4, 5, 6, 7, 8][..],
            &[100, 101][..],
            &[200, 201][..]
        )
    );
}

#[test]
fn full_range_is_squeezed() {
    let mut b = vec![0u8; 8];
    b.copy_from_slice(&[0, 128, 255, 128, 0, 128, 255, 128]);
    let p = to_i420(Pix::Yuyv, &b, 0, 4, 1, Range::Full).unwrap();
    assert_eq!(p.planes().0, [16, 235, 16, 235]);
}

#[test]
fn short_buffers_are_errors() {
    assert!(to_i420(Pix::Yuyv, &[0; 15], 0, 4, 2, Range::Limited).is_err());
    assert!(to_i420(Pix::Nv12, &[0; 10], 0, 4, 2, Range::Limited).is_err());
}

#[test]
fn mjpeg_decodes_and_checks_dimensions() {
    let rgb = image::RgbImage::from_pixel(4, 2, image::Rgb([255, 255, 255]));
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&rgb)
        .unwrap();
    let p = to_i420(Pix::Mjpeg, &jpeg, 0, 4, 2, Range::Limited).unwrap();
    assert_eq!((p.width, p.height), (4, 2));
    assert!(p.planes().0.iter().all(|y| *y >= 230 && *y <= 235));
    assert!(to_i420(Pix::Mjpeg, &jpeg, 0, 2, 2, Range::Limited).is_err());
    assert!(to_i420(Pix::Mjpeg, &jpeg, 0, 6, 2, Range::Limited).is_err());
    assert!(to_i420(Pix::Mjpeg, b"not jpeg", 0, 4, 2, Range::Limited).is_err());
    assert!(to_i420(Pix::Mjpeg, &jpeg, 0, 4096, 2160, Range::Limited).is_err());
}

#[test]
fn self_view_is_small_and_mirrored() {
    let mut p = I420::new(640, 480);
    // Left half white, right half black.
    for row in p.data[..640 * 480].chunks_exact_mut(640) {
        row[..320].fill(235);
        row[320..].fill(16);
    }
    p.data[640 * 480..].fill(128);
    let v = self_view(&p);
    assert_eq!((v.width, v.height), (320, 240));
    assert_eq!(v.data.len(), 320 * 240 * 4);
    // Mirrored: black on the left now.
    assert!(v.data[0] < 10 && v.data[319 * 4] > 245);
}
