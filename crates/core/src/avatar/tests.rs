use super::*;

fn did(seed: u8) -> String {
    identity::did_from_public_key(&[seed; 32])
}

#[test]
fn deterministic_and_bad_did() {
    let a = avatar_art(did(7), false).unwrap();
    assert_eq!(a, avatar_art(did(7), false).unwrap());
    assert_ne!(a, avatar_art(did(8), false).unwrap());
    assert!(avatar_art("did:key:nope".into(), false).is_none());
}

#[test]
fn ranges_and_geometry_for_random_keys() {
    for _ in 0..10_000 {
        let b: [u8; 32] = cryptography::rng::bytes();
        let p = params(&b);
        assert!((5..=16).contains(&p.n));
        assert!(p.k >= 2 && p.k <= (p.n - 1) / 2, "n={} k={}", p.n, p.k);
        assert!((8.0..16.0).contains(&p.cr) && (12.0..22.0).contains(&p.cd));
        assert!((6.0..=12.0).contains(&p.ir));
        assert!(p.hue >= 0.0 && p.hue < 360.0 && p.rot >= 0.0 && p.rot < 360.0 / p.n as f64);
        assert!(p.inner_kind < 3 && p.extra < 4 && p.gold < 4);
        for small in [false, true] {
            let art = art_from_fingerprint(&b, small);
            assert!(!art.strokes.is_empty());
            for s in &art.strokes {
                assert!(s.points.len() >= 4 && s.points.len() % 2 == 0);
                for q in &s.points {
                    // The stroke edge (half width) must stay inside the 100 box too.
                    assert!(*q >= art.stroke_width / 2.0 && *q <= 100.0 - art.stroke_width / 2.0, "{q}");
                }
            }
        }
    }
}

#[test]
fn small_differs_only_by_ring_and_stroke() {
    for _ in 0..500 {
        let b: [u8; 32] = cryptography::rng::bytes();
        let (big, small) = (art_from_fingerprint(&b, false), art_from_fingerprint(&b, true));
        assert_eq!(big.light, small.light);
        assert!(small.stroke_width > big.stroke_width);
        let ring = |a: &AvatarArt| a.strokes.iter().filter(|s| s.alpha == 0.75).count();
        assert_eq!(ring(&small), 0);
        let keep: Vec<_> = big.strokes.iter().filter(|s| s.alpha != 0.75).cloned().collect();
        assert_eq!(keep, small.strokes);
    }
}

#[test]
fn group_art_is_distinct_and_deterministic() {
    let g = group_avatar_art("grp-alpha".into(), false);
    assert_eq!(g, group_avatar_art("grp-alpha".into(), false));
    assert_ne!(g, group_avatar_art("grp-beta".into(), false));
    assert_ne!(group_fingerprint("x"), blake3::derive_key(CTX, b"x"));
    assert!(!group_avatar_art("".into(), true).strokes.is_empty());
}

#[test]
fn colours_are_opaque() {
    for h in [0.0, 90.0, 180.0, 359.0] {
        let (l, d) = colours(h);
        for c in [l.bg, l.fg, d.bg, d.fg] {
            assert_eq!(c >> 24, 0xFF);
        }
    }
}

/// Exact output for synthetic DIDs. Regenerate: `UPDATE_AVATAR_GOLDEN=1 cargo test -p p2pcore avatar::`
/// The playground port is checked against the same file (tools/avatar/golden.mjs).
#[test]
fn golden() {
    let mut out = String::new();
    for seed in [1u8, 2, 3, 5, 8, 13, 21, 34] {
        let d = did(seed);
        for small in [false, true] {
            let f = fingerprint(&d).unwrap();
            let a = art_from_fingerprint(&f, small);
            out += &format!("did {d} small {small} sw {}\n", a.stroke_width);
            out += &format!("fp {}\n", hex::encode(f));
            out += &format!("light {:08x} {:08x} dark {:08x} {:08x}\n", a.light.bg, a.light.fg, a.dark.bg, a.dark.fg);
            for s in &a.strokes {
                out += &format!("s {} {} {}", s.closed as u8, s.gold as u8, s.alpha);
                for q in &s.points {
                    out += &format!(" {q:.3}");
                }
                out += "\n";
            }
        }
    }
    for gid in ["g-0f3a9c", "family", "Weekend hike", "4b1d5e7a9c2f0813"] {
        for small in [false, true] {
            let f = group_fingerprint(gid);
            let a = art_from_fingerprint(&f, small);
            out += &format!("group {gid} small {small} sw {}\n", a.stroke_width);
            out += &format!("fp {}\n", hex::encode(f));
            out += &format!("light {:08x} {:08x} dark {:08x} {:08x}\n", a.light.bg, a.light.fg, a.dark.bg, a.dark.fg);
            for s in &a.strokes {
                out += &format!("s {} {} {}", s.closed as u8, s.gold as u8, s.alpha);
                for q in &s.points {
                    out += &format!(" {q:.3}");
                }
                out += "\n";
            }
        }
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/avatar_golden.txt");
    if std::env::var("UPDATE_AVATAR_GOLDEN").is_ok() {
        std::fs::write(path, &out).unwrap();
    }
    assert_eq!(out, std::fs::read_to_string(path).expect("golden missing; run with UPDATE_AVATAR_GOLDEN=1"));
}
