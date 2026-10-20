use super::*;

fn save(img: &DynamicImage, name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("tinline-photo-{}-{name}", std::process::id()));
    img.save(&p).unwrap();
    p
}

#[test]
fn size_and_grid_of_a_png() {
    // 40x30, left half red, right half blue.
    let mut img = image::RgbImage::new(40, 30);
    for (x, _, p) in img.enumerate_pixels_mut() {
        *p = if x < 20 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) };
    }
    let p = save(&DynamicImage::ImageRgb8(img), "a.png");
    let (w, h, g) = describe(&p, "image/png").unwrap();
    let _ = std::fs::remove_file(&p);
    assert_eq!((w, h), (40, 30));
    assert_eq!(g.len(), PREVIEW_LEN);
    // Column 0 red, column 3 blue, on every row.
    for row in 0..3 {
        let c0 = &g[row * 12..row * 12 + 3];
        let c3 = &g[row * 12 + 9..row * 12 + 12];
        assert!(c0[0] > 200 && c0[2] < 50, "{c0:?}");
        assert!(c3[2] > 200 && c3[0] < 50, "{c3:?}");
    }
}

#[test]
fn not_a_picture() {
    let p = std::env::temp_dir().join(format!("tinline-photo-{}-x.txt", std::process::id()));
    std::fs::write(&p, b"hello").unwrap();
    assert!(describe(&p, "image/png").is_none());
    assert!(describe(&p, "text/plain").is_none());
    let _ = std::fs::remove_file(&p);
}
