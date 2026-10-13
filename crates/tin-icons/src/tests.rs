use kurbo::{ParamCurve, PathEl, Point, QuadBez};

use crate::draw::{sag, view};
use crate::icons::{all, Kind, Role, Tier, GRID, STROKE};
use crate::path::{Path, PathError};
use crate::shape::rounded_rect;

#[test]
fn rounded_rect_matches_shape_ts_byte_for_byte() {
    let fixtures = include_str!("../tests/fixtures/shape.txt");
    let mut n = 0;
    for line in fixtures.lines().filter(|l| !l.is_empty()) {
        let (args, want) = line.split_once('\t').expect("fixture: args<TAB>path");
        let v: Vec<f64> = args.split(' ').map(|x| x.parse().unwrap()).collect();
        let got = rounded_rect(v[7], v[8], v[0], v[1], [v[2], v[3], v[4], v[5]], v[6]).unwrap().d();
        assert_eq!(got, want, "rounded_rect({args})");
        n += 1;
    }
    assert!(n >= 10, "fixtures missing");
}

#[test]
fn parser_reads_relative_shorthand_and_arcs_into_lines_and_cubics_only() {
    let p = Path::svg("m2 2h4v4h-4zM10 10q2 0 2 2t2 2c1 0 1 1 1 1s1 1 1 1M3 20a2 2 0 1 0 4 0").unwrap();
    for el in p.bezier().elements() {
        assert!(!matches!(el, PathEl::MoveTo(pt) if pt.x.is_nan()));
    }
    let d = p.d();
    assert!(d.starts_with("M2 2L6 2L6 6L2 6Z"), "{d}");
    assert!(d.chars().all(|c| !c.is_ascii_alphabetic() || "MLQCZ".contains(c)), "only M L Q C Z: {d}");
    // T reflects the previous Q control (12,10) about (12,12) → (12,14).
    assert!(d.contains("Q12 10 12 12Q12 14 14 14"), "{d}");
}

#[test]
fn parser_rejects_bad_input_with_a_reason() {
    assert!(matches!(Path::svg("L1 1"), Err(PathError::Parse { .. }) | Err(PathError::InvalidSequence { .. })));
    assert!(matches!(Path::svg("M1"), Err(PathError::Parse { .. })));
    assert!(matches!(Path::svg("M1 1A1 1 0 2 0 3 3"), Err(PathError::Parse { what: "expected an arc flag", .. })));
    assert!(matches!(Path::new(vec![PathEl::LineTo(Point::ZERO)]), Err(PathError::InvalidSequence { .. })));
    assert!(matches!(Path::new(vec![PathEl::MoveTo(Point::new(f64::NAN, 0.0))]), Err(PathError::InvalidCoordinate { .. })));
}

#[test]
fn arcs_are_converted_tightly() {
    // A circle of r = 9 drawn with arcs: every on-curve sample stays within the tolerance of the true circle.
    let p = Path::svg(&crate::draw::circle(12.0, 12.0, 9.0)).unwrap();
    for seg in p.bezier().segments() {
        for i in 0..=20 {
            let q = seg.eval(i as f64 / 20.0);
            let r = ((q.x - 12.0).powi(2) + (q.y - 12.0).powi(2)).sqrt();
            assert!((r - 9.0).abs() < 0.005, "radius {r}");
        }
    }
}

#[test]
fn every_icon_fits_the_grid_at_every_tier() {
    let mut bad = Vec::new();
    for t in Tier::ALL {
        for icon in all(t) {
            for part in &icon.parts {
                let pad = if part.role == Role::Fill { 0.0 } else { STROKE / 2.0 };
                let b = part.path.bounds().inflate(pad, pad);
                if !(b.x0 >= -0.01 && b.y0 >= -0.01 && b.x1 <= GRID + 0.01 && b.y1 <= GRID + 0.01) {
                    bad.push(format!("{} @{} {b:?}", icon.name, t.px()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "spill out of the 24 box:\n{}", bad.join("\n"));
}

#[test]
fn names_are_unique_and_tiers_agree_on_the_set() {
    let names: Vec<_> = all(Tier::S24).iter().map(|i| i.name).collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "duplicate icon name");
    for t in [Tier::S16, Tier::S20] {
        assert_eq!(all(t).iter().map(|i| i.name).collect::<Vec<_>>(), names);
    }
}

#[test]
fn the_right_can_is_the_mirror_of_the_left() {
    let call = all(Tier::S24).into_iter().find(|i| i.name == "call").unwrap();
    let cans: Vec<_> = call.parts.iter().filter(|p| p.role != Role::String).collect();
    assert_eq!(cans.len(), 4, "body + mouth per can");
    let (l, r) = (cans[0].path.bounds(), cans[2].path.bounds());
    assert!((l.x0 - (GRID - r.x1)).abs() < 1e-9 && (l.x1 - (GRID - r.x0)).abs() < 1e-9 && l.y0 == r.y0 && l.y1 == r.y1);
}

#[test]
fn the_string_sags_exactly_s_at_its_midpoint() {
    let p = Path::svg(&sag(7.8, 11.0, 16.2, 11.0, 3.4)).unwrap();
    let PathEl::QuadTo(c, e) = p.bezier().elements()[1] else { panic!("string must be one quadratic") };
    let mid = QuadBez::new(Point::new(7.8, 11.0), c, e).eval(0.5);
    assert!((mid.y - 14.4).abs() < 1e-9 && (mid.x - 12.0).abs() < 1e-9, "{mid:?}");
}

#[test]
fn rims_use_the_marks_view_angle() {
    assert!((view() - 30.6_f64.to_radians().sin()).abs() < 1e-12);
    assert!((view() - 0.509).abs() < 0.001);
}

#[test]
fn small_tiers_shrink_only_the_mouths() {
    let at = |t| all(t).into_iter().find(|i| i.name == "mic").unwrap();
    let (s, l) = (at(Tier::S16), at(Tier::S24));
    for (a, b) in s.parts.iter().zip(&l.parts) {
        if a.role == Role::Fill {
            assert!(a.path.bounds().area() < b.path.bounds().area());
        } else {
            assert_eq!(a.path.d(), b.path.d());
        }
    }
}

#[test]
fn every_talking_icon_comes_from_the_mark_and_retired_ones_say_why() {
    let set = all(Tier::S24);
    for name in ["call", "callend", "phone", "mic", "vol", "video", "link", "chat", "send", "users", "bell"] {
        assert_eq!(set.iter().find(|i| i.name == name).unwrap().kind, Kind::Mark, "{name}");
    }
    for i in set.iter().filter(|i| i.kind == Kind::Retire) {
        assert!(i.note.starts_with("Retire:"), "{}", i.name);
    }
}

#[test]
fn generated_files_are_current() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (name, text) in crate::emit::outputs() {
        let old = std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        assert!(old == text, "{name} is stale: run `cargo run -p tin-icons --bin tin-icons-gen`");
    }
}

/// The generated iced module compiles and answers lookups.
#[allow(dead_code)]
mod generated_rs {
    include!("../generated/tin_icons.rs");

    #[test]
    fn lookup_picks_the_nearest_tier() {
        assert_eq!(tier(16.0), 0);
        assert_eq!(tier(20.0), 1);
        assert_eq!(tier(24.0), 2);
        assert!(get("call", 24.0).unwrap().string.contains("Q"));
        assert!(get("nope", 24.0).is_none());
        assert_eq!(NAMES.len(), crate::icons::all(crate::icons::Tier::S24).len());
    }
}
