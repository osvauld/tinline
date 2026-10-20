use super::*;
#[test]
fn recovery_mutations_exclude_transitions_in_both_directions() {
    assert!(mutation_allowed(false, false));
    assert!(!mutation_allowed(true, false));
    assert!(!mutation_allowed(false, true));
    assert!(!mutation_allowed(true, true));
}
#[test]
fn recovery_busy_is_not_core_unavailability() {
    assert_eq!(availability_problem(false), Some(BUSY_UNSUPPORTED));
    assert_eq!(availability_problem(true), None);
}
#[test]
fn profile_link_safety_matches_info_ts() {
    assert_eq!(normalise_link("arjun.example/notes").as_deref(), Some("https://arjun.example/notes"));
    assert_eq!(normalise_link("arjun.example").as_deref(), Some("https://arjun.example/"));
    for s in ["javascript:alert(1)", "https://a:b@host/", "file:///tmp/a", "https://", "https://x\\y", "https://a b", "ftp://x"] {
        assert!(normalise_link(s).is_none(), "{s}");
    }
}
#[test]
fn profile_humanize_and_order_match_info_ts() {
    assert_eq!(humanize("work-title"), "Work title");
    assert_eq!(humanize("workTitle"), "Work Title");
    let f: Map<String, Value> = serde_json::from_value(serde_json::json!({"zeta": 1, "website": "x", "status": "s", "pronouns": "p"})).unwrap();
    assert_eq!(extra_keys(&f), vec!["pronouns", "website", "zeta"]);
}
#[test]
fn profile_crop_math_matches_images_ts() {
    let (w, h) = (800.0, 500.0);
    let c = centred(w, h, VIEW, 1.0);
    let s = cover_scale(w, h, VIEW, 1.0);
    assert!((h * s - VIEW).abs() < 1e-3 && c.oy.abs() < 1e-3 && c.ox < 0.0);
    let far = clamp_crop(Crop { ox: 999.0, oy: -999.0, zoom: 2.0 }, w, h, VIEW);
    let s2 = cover_scale(w, h, VIEW, 2.0);
    assert_eq!(far.ox, 0.0);
    assert!((far.oy - (VIEW - h * s2)).abs() < 1e-3);
    let z = zoom_at(c, 2.0, w, h, VIEW);
    assert!(((VIEW / 2.0 - z.ox) / s2 - (VIEW / 2.0 - c.ox) / s).abs() < 1e-2, "zoom keeps the centre point");
}
#[test]
fn profile_inspect_refuses_with_playground_words() {
    assert!(inspect(b"not a picture").unwrap_err().contains("PNG, JPEG or WebP"));
    assert!(inspect(&vec![0; 2 * 1024 * 1024 + 1]).unwrap_err().contains("up to 2 MB"));
    let mut png = Vec::new();
    image::DynamicImage::new_rgba8(4100, 2).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    assert!(inspect(&png).unwrap_err().contains("4100 \u{d7} 2 pixels"));
}
#[test]
fn profile_render_square_is_at_most_512() {
    let src = image::RgbaImage::new(2000, 1000);
    let out = render_square(&src, centred(2000.0, 1000.0, VIEW, 1.0));
    assert_eq!((out.width(), out.height()), (512, 512));
    let small = image::RgbaImage::new(300, 200);
    let out = render_square(&small, centred(300.0, 200.0, VIEW, 1.0));
    assert_eq!(out.width(), 200);
}
#[test]
fn profile_name_checks_match_profile_ts() {
    assert!(check_name("  ", "name").unwrap_err().contains("can\u{2019}t be empty"));
    assert!(check_name("a\u{202E}b", "name").is_err());
    assert!(check_name(&"é".repeat(129), "name").unwrap_err().contains("256 bytes"));
    assert!(check_text("line\nbreak", "about").is_ok());
}
