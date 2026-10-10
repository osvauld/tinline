use super::*;
use crate::aom::AomEncoder;

fn cfg(w: usize, h: usize) -> EncoderConfig {
    EncoderConfig { width: w, height: h, keyint: 60, ..Default::default() }
}

#[test]
fn round_trip_one_packet_per_frame() {
    let (w, h) = (320, 240);
    let mut enc = AomEncoder::new(&cfg(w, h)).unwrap();
    let mut dec = Decoder::new(1, 4096 * 4096).unwrap();
    let mut pat = Pattern::new(w, h);
    for i in 0..30 {
        let src = pat.next_frame();
        let pkts = enc.encode(&src, false).unwrap();
        assert_eq!(pkts.len(), 1, "frame {i}: realtime config must emit one packet per frame");
        assert_eq!(pkts[0].frame, i);
        assert_eq!(pkts[0].key, i == 0);
        let pics = dec.decode(&pkts[0].data).unwrap();
        assert_eq!(pics.len(), 1, "frame {i}: decoder must not delay");
        assert_eq!((pics[0].width, pics[0].height), (w, h));
        let q = psnr_y(&src, &pics[0]);
        assert!(q > 25.0, "frame {i}: psnr {q}");
    }
}

#[test]
fn forced_keyframe_restarts_decoding() {
    let (w, h) = (320, 240);
    let mut enc = AomEncoder::new(&cfg(w, h)).unwrap();
    let mut pat = Pattern::new(w, h);
    let mut pkts = Vec::new();
    for i in 0..20 {
        pkts.extend(enc.encode(&pat.next_frame(), i == 10).unwrap());
    }
    assert!(pkts[10].key);
    // A receiver that joins (or lost data) at frame 10 decodes from the forced keyframe on.
    let mut dec = Decoder::new(1, 4096 * 4096).unwrap();
    for p in &pkts[10..] {
        assert_eq!(dec.decode(&p.data).unwrap().len(), 1);
    }
}

#[test]
fn garbage_is_an_error_not_a_crash() {
    let mut dec = Decoder::new(1, 4096 * 4096).unwrap();
    let mut seed = 1u32;
    for len in [0usize, 1, 7, 100, 5000] {
        let junk: Vec<u8> = (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (seed >> 16) as u8
            })
            .collect();
        let _ = dec.decode(&junk);
    }
}

#[test]
fn rav1e_holds_frames_back() {
    // Why rav1e is not the call encoder: even in low-latency mode it buffers frames.
    let (w, h) = (320, 240);
    let mut enc = Encoder::new(&cfg(w, h)).unwrap();
    let mut pat = Pattern::new(w, h);
    let held = (0..10).take_while(|_| enc.encode(&pat.next_frame(), false).unwrap().is_empty()).count();
    assert!(held >= 3, "rav1e held {held} frames");
}
