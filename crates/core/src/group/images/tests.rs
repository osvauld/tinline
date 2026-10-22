use super::*;

fn png(w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    if alpha {
        let img = image::RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([
                (x % 256) as u8,
                (y % 256) as u8,
                90,
                if x < w / 2 { 0 } else { 255 },
            ])
        });
        DynamicImage::ImageRgba8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
    } else {
        // Noise, so the JPEG is large and the size loop has work to do.
        let mut seed = 0x2545_f491_u32;
        let img = RgbImage::from_fn(w, h, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            image::Rgb([seed as u8, (seed >> 8) as u8, (seed >> 16) as u8])
        });
        DynamicImage::ImageRgb8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
    }
    out.into_inner()
}

// KIND: unit (public group §5) — a large noisy picture comes out as a JPEG at most 1080 px on
// its long side (shape kept), under 300 KB, with the 4x3 preview; a small one keeps its size;
// transparency is flattened; anything that is not a picture is refused.
#[test]
fn prepare_bounds_the_picture() {
    let p = prepare(&png(2400, 1600, false)).unwrap();
    assert!(p.w <= TARGET_SIDE && p.h <= TARGET_SIDE, "{}x{}", p.w, p.h);
    assert_eq!(p.w * 2, p.h * 3, "aspect kept: {}x{}", p.w, p.h);
    assert!(p.jpeg.len() < TARGET_BYTES, "{} bytes", p.jpeg.len());
    assert_eq!(image::guess_format(&p.jpeg).unwrap(), ImageFormat::Jpeg);
    assert_eq!(p.preview.len(), crate::chat::photo::PREVIEW_LEN);

    let small = prepare(&png(300, 500, true)).unwrap();
    assert_eq!((small.w, small.h), (300, 500));
    assert_eq!(image::guess_format(&small.jpeg).unwrap(), ImageFormat::Jpeg);
    // The transparent half is white now.
    let back = image::load_from_memory(&small.jpeg).unwrap().to_rgb8();
    let px = back.get_pixel(10, 250).0;
    assert!(px.iter().all(|c| *c > 230), "{px:?}");

    assert!(prepare(b"not a picture").is_err());
    assert!(prepare(&[]).is_err());
}
