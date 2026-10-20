use super::*;

#[test]
fn allowlist_needs_mime_and_extension_to_agree() {
    assert_eq!(classify("plan.pdf", "application/pdf"), Class::Open("pdf"));
    assert_eq!(classify("PLAN.PDF", "Application/PDF; x=y"), Class::Open("pdf"));
    assert_eq!(classify("a.jpeg", "image/jpeg"), Class::Image("jpg"));
    assert_eq!(classify("n.md", "text/plain"), Class::Text("md"));
    assert_eq!(classify("r.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"), Class::Open("docx"));
    // disagreement
    assert_eq!(classify("plan.pdf", "image/png"), Class::SaveOnly);
    assert_eq!(classify("plan.pdf", "application/octet-stream"), Class::SaveOnly);
    assert_eq!(classify("photo.png", ""), Class::SaveOnly);
}

#[test]
fn dangerous_types_are_save_only() {
    for (n, m) in [
        ("setup.exe", "application/x-msdownload"),
        ("setup.exe", "application/pdf"),
        ("run.sh", "text/plain"),
        ("run.sh", "application/x-sh"),
        ("x.desktop", "text/plain"),
        ("x.lnk", "application/octet-stream"),
        ("x.bat", "text/plain"),
        ("x.ps1", "text/plain"),
        ("x.py", "text/x-python"),
        ("x.jar", "application/java-archive"),
        ("x.zip", "application/zip"),
        ("x.tar.gz", "application/gzip"),
        ("x.html", "text/html"),
        ("x.htm", "text/html"),
        ("x.svg", "image/svg+xml"),
        ("x.svg", "image/png"),
        ("x.docm", "application/vnd.ms-word.document.macroEnabled.12"),
        ("x.AppImage", "application/octet-stream"),
        ("x.msi", "application/x-msi"),
        ("x.dmg", "application/x-apple-diskimage"),
        ("noext", "application/pdf"),
    ] {
        assert_eq!(classify(n, m), Class::SaveOnly, "{n} {m}");
    }
}

#[test]
fn only_the_last_extension_counts_and_tricks_are_refused() {
    assert_eq!(classify("invoice.pdf.exe", "application/pdf"), Class::SaveOnly);
    assert_eq!(classify("invoice.exe.pdf", "application/pdf"), Class::Open("pdf"));
    assert_eq!(classify("invoice.pdf ", "application/pdf"), Class::SaveOnly);
    assert_eq!(classify("invoice.pdf.", "application/pdf"), Class::SaveOnly);
    assert_eq!(classify("photo\u{202e}gpj.exe", "image/jpeg"), Class::SaveOnly);
    assert_eq!(classify("a\u{0}.pdf", "application/pdf"), Class::SaveOnly);
    assert_eq!(classify("a\n.pdf", "application/pdf"), Class::SaveOnly);
}

#[test]
fn cache_names_are_plain() {
    assert_eq!(cache_name("../../etc/passwd.pdf", "pdf"), "passwd.pdf");
    assert_eq!(cache_name("..\\..\\evil.pdf", "pdf"), "evil.pdf");
    assert_eq!(cache_name(".bashrc.pdf", "pdf"), "bashrc.pdf");
    assert_eq!(cache_name("...pdf", "pdf"), "attachment.pdf");
    assert_eq!(cache_name("", "png"), "attachment.png");
    assert_eq!(cache_name("a/b\\c.exe", "pdf"), "c.pdf");
    assert_eq!(cache_name("na:me*?\"<>|.jpeg", "jpg"), "na_me.jpg");
    assert_eq!(cache_name("Floor plan \u{2013} 3rd.pdf", "pdf"), "Floor plan _ 3rd.pdf");
    let long = format!("{}.pdf", "a".repeat(500));
    assert!(cache_name(&long, "pdf").chars().count() <= 64);
    for n in ["a\u{0}b.pdf", "a\u{202e}b.pdf", "/", ".", ".."] {
        let c = cache_name(n, "pdf");
        assert!(!c.contains(['/', '\\', '\u{0}', '\u{202e}']) && !c.starts_with('.') && c.ends_with(".pdf"), "{c}");
    }
}

#[test]
fn hash_dir_is_a_plain_name() {
    assert_eq!(hash_dir("../../x"), "x");
    assert_eq!(hash_dir(""), "x");
    assert_eq!(hash_dir("ab12/.."), "ab12");
}

#[test]
fn image_size_limits() {
    assert!(dimensions_ok(4000, 3000));
    assert!(!dimensions_ok(12_001, 10));
    assert!(!dimensions_ok(10_000, 10_000));
    assert!(!dimensions_ok(0, 5));
}

#[test]
fn decode_scales_down_and_refuses_bombs() {
    let dir = std::env::temp_dir().join(format!("tinline-media-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = |name: &str, w: u32, h: u32| {
        let p = dir.join(name);
        let f = std::fs::File::create(&p).unwrap();
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        e.set_color(png::ColorType::Grayscale);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&vec![128u8; (w * h) as usize]).unwrap();
        drop(wr);
        p
    };
    let big = png("big.png", 1000, 500);
    let px = decode_scaled(&big, THUMB_W, THUMB_H).unwrap();
    assert_eq!((px.w, px.h), (640, 320));
    assert_eq!(px.rgba.len(), 640 * 320 * 4);
    let small = png("small.png", 40, 30);
    assert_eq!(decode_scaled(&small, THUMB_W, THUMB_H).unwrap().w, 40);
    // a header that claims a huge picture is refused before anything is allocated
    let bomb = png("bomb.png", 13_000, 2);
    assert!(decode_scaled(&bomb, THUMB_W, THUMB_H).is_err());
    let junk = dir.join("junk.png");
    std::fs::write(&junk, b"not an image").unwrap();
    assert!(decode_scaled(&junk, THUMB_W, THUMB_H).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
