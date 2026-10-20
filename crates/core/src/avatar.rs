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
mod tests;
