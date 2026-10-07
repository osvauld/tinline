use audio::*;
fn main() {
    for hz in [440.0f32, 550.0, 660.0, 770.0, 880.0, 1000.0] {
        let (mut tx, mut rx) = (Sender::new(48_000).unwrap(), Receiver::new().unwrap());
        let mut tone = Tone::new(hz, 10_000.0);
        let (mut inp, mut out) = (Vec::new(), Vec::new());
        let mut f = [0i16; FRAME];
        let mut o = [0i16; FRAME];
        for i in 0..400u64 {
            tone.next_frame(&mut f);
            inp.extend_from_slice(&f);
            let d = tx.encode(&f).unwrap();
            rx.push(&d, i * 20);
            rx.pull(&mut o, i * 20);
            out.extend_from_slice(&o);
        }
        println!("{hz:>6} Hz  in rms {:>6.0}  out rms {:>6.0}  out freq {:?}", rms(&inp[48000..]), rms(&out[48000..]), estimate_frequency(&out[48000..]));
    }
}
