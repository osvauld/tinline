// The call field's fragment shader: WGSL port of the playground's lib/calls/field/gl.ts (same noise, same
// Gaussian coverage, same OKLab mean with chroma restore, same calm patches and dither). Fog and Clouds.
// Uniform slots: 0 res.xy tau warp | 1 cap ws nr clouds | 2 bg.xyz gain | 3 scrimK lit srgb - |
// 4,5 calm patches (x y rx ry) | 6 widget origin (px) | 7..11 lobes (x y sigma alpha) | 12..16 lobe OKLab.
struct U { v: array<vec4<f32>, 17> }
@group(0) @binding(0) var<uniform> u: U;

@vertex fn vs(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let xy = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.))[id];
    return vec4(xy, 0., 1.);
}

fn rot(p: vec2<f32>) -> vec2<f32> { return vec2(.8 * p.x - .6 * p.y, .6 * p.x + .8 * p.y); }
fn h23(p0: vec3<f32>) -> vec2<f32> {
    var p = fract(p0 * vec3(.1031, .1030, .0973)); p += dot(p, p.yzx + 33.33);
    return fract((p.xx + p.yz) * p.zy) * 2. - 1.;
}
fn vn(p: vec3<f32>) -> vec2<f32> {
    let i = floor(p); let f = fract(p); let q = f * f * f * (f * (f * 6. - 15.) + 10.);
    let a = mix(mix(h23(i), h23(i + vec3(1., 0., 0.)), q.x), mix(h23(i + vec3(0., 1., 0.)), h23(i + vec3(1., 1., 0.)), q.x), q.y);
    let b = mix(mix(h23(i + vec3(0., 0., 1.)), h23(i + vec3(1., 0., 1.)), q.x), mix(h23(i + vec3(0., 1., 1.)), h23(i + vec3(1., 1., 1.)), q.x), q.y);
    return mix(a, b, q.z);
}
fn ok2lin(c: vec3<f32>) -> vec3<f32> {
    var l = c.x + .3963377774 * c.y + .2158037573 * c.z;
    var m = c.x - .1055613458 * c.y - .0638541728 * c.z;
    var s = c.x - .0894841775 * c.y - 1.2914855480 * c.z;
    l = l * l * l; m = m * m * m; s = s * s * s;
    return vec3(4.0767416621 * l - 3.3077115913 * m + .2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - .3413193965 * s, -.0041960863 * l - .7034186147 * m + 1.7076147010 * s);
}
fn lin2s(c0: vec3<f32>) -> vec3<f32> {
    let c = clamp(c0, vec3(0.), vec3(1.));
    return select(1.055 * pow(c, vec3(1. / 2.4)) - .055, 12.92 * c, c < vec3(.0031308));
}
fn s2lin(c0: vec3<f32>) -> vec3<f32> {
    let c = clamp(c0, vec3(0.), vec3(1.));
    return select(pow((c + .055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(.04045));
}
fn hs(p0: vec3<f32>) -> f32 { var p = fract(p0 * .1031); p += dot(p, p.zyx + 31.32); return fract((p.x + p.y) * p.z); }
fn vn1(p: vec3<f32>) -> f32 {
    let i = floor(p); let f = fract(p); let q = f * f * f * (f * (f * 6. - 15.) + 10.);
    return mix(mix(mix(hs(i), hs(i + vec3(1., 0., 0.)), q.x), mix(hs(i + vec3(0., 1., 0.)), hs(i + vec3(1., 1., 0.)), q.x), q.y),
               mix(mix(hs(i + vec3(0., 0., 1.)), hs(i + vec3(1., 0., 1.)), q.x), mix(hs(i + vec3(0., 1., 1.)), hs(i + vec3(1., 1., 1.)), q.x), q.y), q.z);
}
fn fbm(p0: vec3<f32>) -> f32 {
    var p = p0; var a = .5; var s = 0.;
    for (var i = 0; i < 4; i++) { s += a * vn1(p); p = vec3(rot(p.xy) * 2.03 + vec2(11.3, 7.1), p.z * 1.45 + 3.7); a *= .5; }
    return s / .9375;
}
fn h22(p: vec2<f32>) -> vec2<f32> { var q = fract(vec3(p.x, p.y, p.x) * vec3(.1031, .1030, .0973)); q += dot(q, q.yzx + 33.33); return fract((q.xx + q.yz) * q.zy); }
fn bump(p: vec2<f32>, t: f32) -> f32 {
    let i = floor(p); let f = fract(p); var d = 4.;
    for (var y = -1; y <= 1; y++) { for (var x = -1; x <= 1; x++) {
        let g = vec2(f32(x), f32(y)); let o = .5 + .38 * sin(t + 6.2832 * h22(i + g)); let r = g + o - f; d = min(d, dot(r, r));
    } }
    return 1. - sqrt(d);
}
fn dens(p: vec3<f32>) -> f32 { return .6 * bump(p.xy, p.z * 1.4) + .24 * bump(rot(p.xy) * 2.1 + 5., p.z * 1.9 + 2.) + .16 * fbm(p * 1.3); }

@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let res = u.v[0].xy; let short = min(res.x, res.y); let tau = u.v[0].z; let warp = u.v[0].w;
    let cap = u.v[1].x; let ws = u.v[1].y; let nr = u.v[1].z;
    let bg = u.v[2].xyz; let gain = u.v[2].w;
    let fc = pos.xy - u.v[6].xy;                 // pixel in the widget, y down
    let p = (fc - .5 * res) / short;
    var lab = bg;
    if (u.v[1].w > .5) {
        // CLOUDS: lobes steer where the cloud mass swells (coverage S), the fbm decides the shape of the puffs.
        var S = 0.; var W = 0.; var ab = vec3(0.); var cs = 0.;
        for (var i = 0; i < 5; i++) {
            let l = u.v[7 + i]; let d = p - l.xy; let a = l.w * exp(-dot(d, d) / (2. * l.z * l.z));
            let w = select(a, a * a, bg.x < .5) + 1e-9; S += a; W += w; ab += w * u.v[12 + i].xyz; cs += w * length(u.v[12 + i].yz);
        }
        let Sc = 1. - exp(-S);
        var m = ab / W; cs /= W; let cm = length(m.yz); let ct = mix(cm, cs, select(1., .25, gain > 1.01));
        if (cm > 1e-4) { m = vec3(m.x, m.yz * (ct / cm)); }
        let z = tau * nr * .5;
        let wv = vn(vec3(p * 1.15 + 3.7, z * .8)) * (.07 + warp * .9);
        let dr = vec2(.045, -.018) * tau;
        let cp = vec3((p + wv) * 1.15 + dr, z);
        let tl = vec2(-.55, -.83) * .16;
        let dn = dens(cp) + .26 * Sc;
        let dl = dens(cp + vec3(tl, 0.)) + .26 * Sc;
        let puff = smoothstep(.52, .70, dn);
        let lit = clamp(.5 - (dl - dn) * 3.2, 0., 1.);
        let dk = step(bg.x, .5);
        let sky = mix(bg, m, cap * mix(.85, .5, dk) * Sc);
        var cc = mix(sky, m, .7);
        cc = vec3(cc.x + (lit - .45) * u.v[3].y * 1.8, cc.yz * (1. - .12 * lit + .12 * (1. - lit)));
        lab = mix(sky, cc, puff * (.30 + .70 * Sc) * mix(1., .72, dk));
    } else {
        let z = tau * nr;
        var w = vn(vec3(p * ws, z)) * .55; let p2 = rot(p);
        w += vn(vec3(p2 * ws * 2.07 + 7.3, z * 1.3 + 3.)) * .30; let p3 = rot(p2);
        w += vn(vec3(p3 * ws * 4.13 + 13.1, z * 1.7 + 9.)) * .15;
        let q = p + warp * w;
        // Coverage S sums the lobes, colour is their weighted mean; light restores the mean's chroma, dark mostly doesn't.
        var S = 0.; var W = 0.; var ab = vec3(0.); var cs = 0.;
        for (var i = 0; i < 5; i++) {
            let l = u.v[7 + i]; let d = q - l.xy; let a = l.w * exp(-dot(d, d) / (2. * l.z * l.z));
            S += a; W += a + 1e-9; ab += (a + 1e-9) * u.v[12 + i].xyz; cs += (a + 1e-9) * length(u.v[12 + i].yz);
        }
        if (S > 1e-5) {
            var m = ab / W; cs /= W; let cm = length(m.yz); let ct = mix(cm, cs, select(1., .25, gain > 1.01));
            if (cm > 1e-4) { m = vec3(m.x, m.yz * (ct / cm)); }
            lab = mix(bg, m, cap * (1. - exp(-S)));
        }
    }
    // Calm patches behind the text block and the controls (flat inside, soft edge).
    var sc = 0.;
    for (var i = 0; i < 2; i++) { let d = (p - u.v[4 + i].xy) / u.v[4 + i].zw; sc = max(sc, 1. - smoothstep(0., 1., length(d))); }
    lab = mix(lab, bg, u.v[3].x * sc);
    lab = vec3(lab.x, lab.yz * gain);
    var rgb = lin2s(ok2lin(lab));
    let gl = vec2(fc.x, res.y - fc.y);           // the browser's bottom-up gl_FragCoord, for the same dither
    let n = fract(52.9829189 * fract(dot(gl, vec2(.06711056, .00583715))));
    rgb += (n - .5) / 255.;
    // An sRGB target encodes on write: hand it linear light.
    return vec4(select(rgb, s2lin(rgb), u.v[3].z > .5), 1.);
}
