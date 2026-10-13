//! Equal-gain PCM mixing with the existing soft limiter. No heap allocation.
/// Sum available source samples, with silence for missing samples.
pub fn mix(frames: &[&[i16]], out: &mut [i16]) {
    for (i, o) in out.iter_mut().enumerate() {
        let sum: i32 = frames
            .iter()
            .map(|f| f.get(i).copied().unwrap_or(0) as i32)
            .sum();
        *o = limit(sum);
    }
}
/// Apply the identical limiter to bounded caller-accumulated PCM sums.
/// Missing sums are silence. The caller must keep integer accumulation within i32.
pub fn mix_sums(sums: &[i32], out: &mut [i16]) {
    for (i, o) in out.iter_mut().enumerate() {
        *o = limit(sums.get(i).copied().unwrap_or(0));
    }
}
fn limit(s: i32) -> i16 {
    const MIX_KNEE: f32 = 24_576.;
    const HEADROOM: f32 = i16::MAX as f32 - MIX_KNEE;
    let a = s.unsigned_abs() as f32;
    if a <= MIX_KNEE {
        s as i16
    } else {
        let y = MIX_KNEE + HEADROOM * ((a - MIX_KNEE) / HEADROOM).tanh();
        (y.min(i16::MAX as f32) as i16) * s.signum() as i16
    }
}
