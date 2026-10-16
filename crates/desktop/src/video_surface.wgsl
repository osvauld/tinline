// The peer's video: Y, U, V planes (R8) to RGB, BT.601 limited range like video::convert::i420_to_rgba.
// Uniform: x y = the picture's size as a fraction of the widget (centred), z = target is sRGB.
struct U { v: array<vec4<f32>, 2> }
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var ty: texture_2d<f32>;
@group(0) @binding(3) var tu: texture_2d<f32>;
@group(0) @binding(4) var tv: texture_2d<f32>;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex fn vs(@builtin(vertex_index) id: u32) -> VOut {
    let c = vec2<f32>(f32(id & 1u), f32(id >> 1u));
    var o: VOut;
    o.pos = vec4((c * 2. - 1.) * u.v[0].xy * vec2(1., -1.), 0., 1.);
    o.uv = c;
    return o;
}

@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let y = 1.164383 * (textureSampleLevel(ty, smp, i.uv, 0.).r - 16. / 255.);
    let d = textureSampleLevel(tu, smp, i.uv, 0.).r - 128. / 255.;
    let e = textureSampleLevel(tv, smp, i.uv, 0.).r - 128. / 255.;
    let s = clamp(vec3(y + 1.596027 * e, y - 0.391762 * d - 0.812968 * e, y + 2.017232 * d), vec3(0.), vec3(1.));
    // The surface re-encodes to sRGB when it is an sRGB format, so hand it linear light then.
    let lin = select(pow((s + .055) / 1.055, vec3(2.4)), s / 12.92, s <= vec3(.04045));
    return vec4(select(s, lin, u.v[0].z > .5), 1.);
}
