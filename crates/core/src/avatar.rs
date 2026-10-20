//! Fingerprint avatars ("sacred geometry"): a small mandala drawn from a key fingerprint.
//! Spec: docs/design/FINGERPRINT-AVATARS.md. Pure maths, no I/O. The playground port
//! (docs/design/playground/src/lib/ui/fingerprint.ts) must reproduce `avatar_golden.txt` exactly.
//!
//! Recognition, not verification: two keys can in principle draw alike, only the safety number proves who someone is.

use std::f64::consts::PI;

const CTX: &str = "tinline/avatar/v1";
const GROUP_CTX: &str = "tinline/avatar/v1/group";
const C: f64 = 50.0;
const R: f64 = 42.0;
/// Segments used to sample a full circle; arcs use `ARC_SEGS`.
const CIRCLE_SEGS: usize = 48;
const ARC_SEGS: usize = 24;
/// Below this many logical pixels the app asks for the small variant.
pub const SMALL_BELOW_PX: f32 = 40.0;
const STROKE: f32 = 1.8;
const STROKE_SMALL: f32 = 3.4;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AvatarStroke {
    /// x,y pairs in 100 x 100 box units, already rotated.
    pub points: Vec<f32>,
    pub closed: bool,
    /// Drawn in the app's gold token instead of the avatar foreground.
    pub gold: bool,
    pub alpha: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct AvatarColours {
    /// 0xAARRGGBB
    pub bg: u32,
    pub fg: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AvatarArt {
    pub strokes: Vec<AvatarStroke>,
    /// Stroke width in box units.
    pub stroke_width: f32,
    pub light: AvatarColours,
    pub dark: AvatarColours,
}

/// The 32-byte fingerprint of a DID, or None for a malformed DID.
pub fn fingerprint(did: &str) -> Option<[u8; 32]> {
    let pk = identity::public_key_from_did(did)?;
    Some(blake3::derive_key(CTX, &pk))
}

/// The 32-byte fingerprint of a group id (its own context, so a group never draws like a person).
pub fn group_fingerprint(gid: &str) -> [u8; 32] {
    blake3::derive_key(GROUP_CTX, gid.as_bytes())
}

/// The parameters the bytes select.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub hue: f64,
    pub n: usize,
    pub k: usize,
    pub cr: f64,
    pub cd: f64,
    pub inner_kind: u8,
    pub ir: f64,
    pub extra: u8,
    pub gold: u8,
    pub rot: f64,
}

pub fn params(b: &[u8; 32]) -> Params {
    let n = 5 + (b[1] % 12) as usize;
    let kmax = (n - 1) / 2;
    Params {
        hue: b[0] as f64 / 256.0 * 360.0,
        n,
        k: 2 + (b[2] as usize % (kmax - 1)),
        cr: 8.0 + (b[3] % 8) as f64,
        cd: 12.0 + (b[4] % 10) as f64,
        inner_kind: b[5] % 3,
        ir: 6.0 + ((b[5] >> 2) % 4) as f64 * 2.0,
        extra: b[6] % 4,
        gold: b[7] % 4,
        rot: b[8] as f64 / 256.0 * (360.0 / n as f64),
    }
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn pt(r: f64, i: usize, m: usize, off: f64) -> (f64, f64) {
    let a = ((i as f64 / m as f64) * 360.0 + off - 90.0) * (PI / 180.0);
    (C + r * a.cos(), C + r * a.sin())
}

type Path = (Vec<(f64, f64)>, bool);

/// Closed sub-paths of the star polygon {m/step}; gcd(m, step) of them (compound stars).
fn poly(r: f64, m: usize, step: usize, off: f64) -> Vec<Path> {
    let g = gcd(m, step);
    (0..g).map(|s| ((0..m / g).map(|j| pt(r, (s + j * step) % m, m, off)).collect(), true)).collect()
}

fn circle(cx: f64, cy: f64, r: f64) -> Path {
    ((0..CIRCLE_SEGS).map(|i| {
        let a = i as f64 / CIRCLE_SEGS as f64 * 2.0 * PI;
        (cx + r * a.cos(), cy + r * a.sin())
    }).collect(), true)
}

/// SVG arc `A r r 0 0 1` from p0 to p1 (small arc, positive sweep), sampled, excluding the end point.
fn arc(p0: (f64, f64), p1: (f64, f64), r: f64) -> Vec<(f64, f64)> {
    let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
    let d = (dx * dx + dy * dy).sqrt();
    let h = (r * r - d * d / 4.0).max(0.0).sqrt();
    let (ux, uy) = (dx / d, dy / d);
    let c = ((p0.0 + p1.0) / 2.0 - h * uy, (p0.1 + p1.1) / 2.0 + h * ux);
    let t0 = (p0.1 - c.1).atan2(p0.0 - c.0);
    let t1 = (p1.1 - c.1).atan2(p1.0 - c.0);
    let mut dt = t1 - t0;
    while dt < 0.0 {
        dt += 2.0 * PI;
    }
    (0..ARC_SEGS).map(|i| {
        let t = t0 + dt * i as f64 / ARC_SEGS as f64;
        (c.0 + r * t.cos(), c.1 + r * t.sin())
    }).collect()
}

/// Strokes (layer index, alpha, path) before rotation. Layers: 0 star, 1 ring, 2 inner, 3 extra.
fn layers(p: &Params, small: bool) -> Vec<(u8, f32, Path)> {
    let (n, phi) = (p.n, (1.0 + 5f64.sqrt()) / 2.0);
    let half = 180.0 / n as f64;
    let mut out: Vec<(u8, f32, Path)> = Vec::new();
    out.extend(poly(R, n, p.k, 0.0).into_iter().map(|s| (0, 1.0, s)));
    let ring: Vec<(f64, f64)> = (0..n).map(|i| pt(p.cd, i, n, half)).collect();
    if !small {
        out.extend(ring.iter().map(|c| (1, 0.75, circle(c.0, c.1, p.cr))));
    }
    match p.inner_kind {
        0 => out.push((2, 1.0, circle(C, C, p.ir))),
        1 => out.extend(poly(p.ir, n, 1, half).into_iter().map(|s| (2, 1.0, s))),
        _ => out.extend(poly(p.ir, n, 2.max((n - 1) / 2), half).into_iter().map(|s| (2, 1.0, s))),
    }
    match p.extra {
        1 => {
            out.extend(poly(p.cd, n, 1, half).into_iter().map(|s| (3, 0.7, s)));
            out.extend(ring.iter().map(|c| (3, 0.7, (vec![(C, C), *c], false))));
        }
        2 => {
            let pr = p.cd * 0.9;
            for i in 0..n {
                let tip = pt(pr, i, n, 0.0);
                let mut v = arc((C, C), tip, pr * 0.62);
                v.extend(arc(tip, (C, C), pr * 0.62));
                out.push((3, 0.7, (v, true)));
            }
        }
        3 => {
            out.push((3, 0.7, circle(C, C, R / phi)));
            out.push((3, 0.7, circle(C, C, R / (phi * phi))));
        }
        _ => {}
    }
    out
}

fn oklch(l: f64, c: f64, h_deg: f64) -> u32 {
    let h = h_deg.to_radians();
    let (a, b) = (c * h.cos(), c * h.sin());
    let l_ = l + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = l - 0.0894841775 * a - 1.2914855480 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let lin = [
        4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3,
        -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3,
        -0.0041960863 * l3 - 0.7034186147 * m3 + 1.7076147010 * s3,
    ];
    let ch = |x: f64| {
        let x = x.clamp(0.0, 1.0);
        let v = if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u32
    };
    0xFF00_0000 | (ch(lin[0]) << 16) | (ch(lin[1]) << 8) | ch(lin[2])
}

pub fn colours(hue: f64) -> (AvatarColours, AvatarColours) {
    (
        AvatarColours { bg: oklch(0.94, 0.035, hue), fg: oklch(0.42, 0.09, hue) },
        AvatarColours { bg: oklch(0.34, 0.05, hue), fg: oklch(0.88, 0.07, hue) },
    )
}

/// The art for fingerprint `b`.
pub fn art_from_fingerprint(b: &[u8; 32], small: bool) -> AvatarArt {
    let p = params(b);
    let (s, c) = p.rot.to_radians().sin_cos();
    let strokes = layers(&p, small)
        .into_iter()
        .map(|(layer, alpha, (pts, closed))| AvatarStroke {
            points: pts.iter().flat_map(|&(x, y)| {
                let (dx, dy) = (x - C, y - C);
                [(C + dx * c - dy * s) as f32, (C + dx * s + dy * c) as f32]
            }).collect(),
            closed,
            gold: layer == p.gold,
            alpha,
        })
        .collect();
    let (light, dark) = colours(p.hue);
    AvatarArt { strokes, stroke_width: if small { STROKE_SMALL } else { STROKE }, light, dark }
}

/// The avatar for a DID. `small` = drawn below `SMALL_BELOW_PX` logical pixels. None for a malformed DID
/// (the app keeps its current avatar).
#[uniffi::export]
pub fn avatar_art(did: String, small: bool) -> Option<AvatarArt> {
    Some(art_from_fingerprint(&fingerprint(&did)?, small))
}

/// A group's avatar from its group id: same byte map and geometry as a person; the app draws it in a rounded square
/// (radius 28% of the size) instead of a circle. Group ids are ordinary strings, so this never fails.
#[uniffi::export]
pub fn group_avatar_art(gid: String, small: bool) -> AvatarArt {
    art_from_fingerprint(&group_fingerprint(&gid), small)
}

#[cfg(test)]
mod tests {
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
}
