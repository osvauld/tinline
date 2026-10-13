//! Independent oracle for the existing equal-gain soft limiter, including pre-summed PCM.
use super::*;
fn reference(s: i32) -> i16 {
    let a = s.unsigned_abs() as f32;
    if a <= 24_576. {
        s as i16
    } else {
        let y = 24_576. + 8_191. * ((a - 24_576.) / 8_191.).tanh();
        (y.min(32_767.) as i16) * s.signum() as i16
    }
}
#[test]
fn summed_pcm_matches_limiter_at_edges_and_extremes() {
    let sums = [
        i32::MIN,
        -393216,
        -32768,
        -24577,
        -24576,
        -1,
        0,
        1,
        24576,
        24577,
        32767,
        393204,
        i32::MAX,
    ];
    let mut out = [0_i16; 13];
    mix_sums(&sums, &mut out);
    assert_eq!(out, sums.map(reference));
    mix_sums(&[], &mut out);
    assert_eq!(out, [0; 13]);
}
#[test]
fn summed_pcm_is_bit_exact_for_one_three_and_twelve_speakers() {
    for n in [1, 3, 12] {
        let frames: [[i16; FRAME]; 12] = std::array::from_fn(|s| {
            std::array::from_fn(|i| ((s * 9119 + i * 3571) % 65536) as u16 as i16)
        });
        let sums: [i32; FRAME] =
            std::array::from_fn(|i| frames[..n].iter().map(|f| f[i] as i32).sum());
        let refs: Vec<&[i16]> = frames[..n].iter().map(|f| &f[..]).collect();
        let mut out = [0_i16; FRAME];
        let mut original = [0_i16; FRAME];
        mix_sums(&sums, &mut out);
        mix(&refs, &mut original);
        assert_eq!(out, sums.map(reference));
        assert_eq!(out, original);
    }
}
