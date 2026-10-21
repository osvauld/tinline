use super::*;

#[test]
fn bytes_cross_like_vec() {
    use uniffi::{Lift, Lower, RustBuffer};
    type T = crate::UniFfiTag;
    let data: Vec<u8> = (0..640 * 480 * 3 / 2).map(|i| i as u8).collect();
    let time = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        for _ in 0..20 {
            f();
        }
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    };
    let wire = <Vec<u8> as Lower<T>>::lower_into_rust_buffer(data.clone()).destroy_into_vec();
    assert_eq!(<Bytes as Lower<T>>::lower_into_rust_buffer(Bytes(data.clone())).destroy_into_vec(), wire);
    assert_eq!(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap().0, data);
    assert!(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire[..wire.len() - 1].to_vec())).is_err());
    let frame = VideoFrame { width: 640, height: 480, data: Bytes(data.clone()), seq: 1 };
    let mut r = &<VideoFrame as Lower<T>>::lower_into_rust_buffer(frame).destroy_into_vec()[..];
    assert_eq!(<VideoFrame as Lift<T>>::try_read(&mut r).unwrap().data.0, data);
    let lift_vec = time(&mut || drop(<Vec<u8> as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap()));
    let lift_bytes = time(&mut || drop(<Bytes as Lift<T>>::try_lift(RustBuffer::from_vec(wire.clone())).unwrap()));
    let lower_vec = time(&mut || <Vec<u8> as Lower<T>>::lower(data.clone()).destroy());
    let lower_bytes = time(&mut || <Bytes as Lower<T>>::lower(Bytes(data.clone())).destroy());
    println!("640x480 I420 ms (incl. a clone): lift Vec<u8> {lift_vec:.3} Bytes {lift_bytes:.3}, lower Vec<u8> {lower_vec:.3} Bytes {lower_bytes:.3}");
}

#[test]
fn rgba_pull_converts() {
    let mut pic = I420::new(4, 2);
    pic.data.fill(128);
    pic.data[..8].fill(235);
    let f = to_rgba(VideoFrame { width: 4, height: 2, data: pic.data.into(), seq: 7 });
    assert_eq!((f.width, f.height, f.seq, f.data.len()), (4, 2, 7, 32));
    assert!(f.data.chunks(4).all(|p| p == [255, 255, 255, 255]));
}

#[test]
fn peer_cap_limits_the_ladder() {
    let l = LadderConfig::default();
    assert_eq!(max_rung(&l, 1280 * 720), 3);
    assert_eq!(max_rung(&l, 640 * 360), 1);
    assert_eq!(max_rung(&l, 1), 0);
}

#[test]
fn offer_has_av1_and_vp8_only_where_built_in() {
    let c = codecs::local_offer_codecs();
    assert_eq!(c.decode[0], VideoCodec::Av1);
    assert_eq!(c.decode.contains(&VideoCodec::Vp8), cfg!(feature = "vp8"));
    // Hosts encode VP8 in software (libvpx) when built with it, after AV1: only a receiver
    // with a hardware VP8 decoder makes `send_codec` pick it. Nothing on a host is hardware.
    let want = if cfg!(feature = "vp8") { vec![VideoCodec::Av1, VideoCodec::Vp8] } else { vec![VideoCodec::Av1] };
    assert_eq!(c.encode, want);
    assert!(c.hw_decode.is_empty() && c.hw_encode.is_empty());
    assert_eq!(codecs::new_encoder(codecs::EncoderChoice { codec: VideoCodec::Vp8, hardware: false }, &EncoderConfig::default()).is_ok(), cfg!(feature = "vp8"));
    assert_eq!(codecs::new_decoder(VideoCodec::Vp8, false, MAX_PIXELS).is_ok(), cfg!(feature = "vp8"));
}

/// The desktop's VP8 send path end to end without a network: `codecs::new_encoder(Vp8)` output
/// goes through the receive loop's VP8 decoder (design doc acceptance test 3).
#[cfg(feature = "vp8")]
#[test]
fn desktop_vp8_encoder_feeds_the_vp8_receive_path() {
    let (w, h) = (320, 240);
    let cfg = EncoderConfig { width: w, height: h, keyint: 300, ..Default::default() };
    let mut enc = codecs::new_encoder(codecs::EncoderChoice { codec: VideoCodec::Vp8, hardware: false }, &cfg).unwrap();
    let mut cam = video::Pattern::new(w, h);
    let pipe = Arc::new(Pipe::default());
    let (tx, rx) = mpsc::channel(32);
    for i in 0..20u32 {
        let pkts = enc.encode(&cam.next_frame(), false).unwrap();
        assert_eq!(pkts.len(), 1);
        assert_eq!(pkts[0].key, i == 0);
        tx.try_send(Frame { group: 0, index: i, capture_us: 0, payload: pkts[0].data.clone() }).unwrap();
    }
    drop(tx);
    decode_loop(pipe.clone(), rx, VideoCodec::Vp8);
    let st = pipe.stats.lock().clone();
    assert_eq!((st.frames_decoded, st.decode_errors), (20, 0));
    assert_eq!((st.recv_width, st.recv_height), (w as u32, h as u32));
}

/// The receive path with the negotiated codec VP8: frames from the video crate's libvpx fixture
/// (see `video::vpx` tests for how it was made) come out as pictures and stats.
#[cfg(feature = "vp8")]
#[test]
fn vp8_frames_decode_on_the_receive_path() {
    const IVF: &[u8] = include_bytes!("../../../video/testdata/vp8_96x64.ivf");
    let mut payloads = Vec::new();
    let mut at = 32;
    while at + 12 <= IVF.len() {
        let n = u32::from_le_bytes(IVF[at..at + 4].try_into().unwrap()) as usize;
        payloads.push(IVF[at + 12..at + 12 + n].to_vec());
        at += 12 + n;
    }
    let pipe = Arc::new(Pipe::default());
    let (tx, rx) = mpsc::channel(payloads.len());
    for (i, payload) in payloads.into_iter().enumerate() {
        tx.try_send(Frame { group: 0, index: i as u32, capture_us: 0, payload }).unwrap();
    }
    drop(tx);
    decode_loop(pipe.clone(), rx, VideoCodec::Vp8);
    let st = pipe.stats.lock().clone();
    assert_eq!((st.frames_decoded, st.decode_errors), (8, 0));
    assert_eq!((st.recv_width, st.recv_height), (96, 64));
    let out = pipe.output.lock().take().unwrap();
    assert_eq!((out.width, out.height, out.seq), (96, 64, 8));
}

#[test]
fn only_mediacodec_decoders_are_pipelined() {
    // Android VP8; AV1 (rav1d) and desktop libvpx not.
    assert!(!codecs::pipelined_decoder(proto::VideoCodec::Av1));
    assert_eq!(
        codecs::pipelined_decoder(proto::VideoCodec::Vp8),
        cfg!(all(target_os = "android", feature = "mediacodec"))
    );
}
