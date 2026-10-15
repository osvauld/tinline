use super::*;
use std::path::Path;

fn tmp(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("tinline-voice-{}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    d.join(name).to_string_lossy().into_owned()
}

/// `audio::estimate_frequency` assumes 48 kHz input.
fn freq(pcm: &[i16]) -> f32 {
    audio::estimate_frequency(pcm).unwrap() * VOICE_RATE as f32 / audio::RATE as f32
}

fn sine(hz: f32, ms: u32, amp: f32) -> Vec<i16> {
    (0..ms * 16)
        .map(|i| (amp * (2.0 * std::f32::consts::PI * hz * i as f32 / 16_000.0).sin()) as i16)
        .collect()
}

fn record(path: &str, pcm: &[i16]) -> VoiceInfo {
    let r = VoiceRecorder::start(path.into()).unwrap();
    for c in pcm.chunks(441) {
        r.push(c.to_vec()).unwrap();
    }
    r.finish().unwrap()
}

fn read_all(path: &str) -> Vec<i16> {
    let d = VoiceDecoder::open(path.into()).unwrap();
    let mut all = Vec::new();
    loop {
        let c = d.read(1000).unwrap();
        if c.is_empty() {
            return all;
        }
        all.extend(c);
    }
}

#[test]
fn round_trip_keeps_duration_and_pitch() {
    let p = tmp("rt.opus");
    let info = record(&p, &sine(440.0, 3000, 12000.0));
    assert_eq!(info.duration_ms, 3000);
    let d = VoiceDecoder::open(p.clone()).unwrap();
    assert_eq!(d.duration_ms(), 3000);
    let pcm = read_all(&p);
    assert_eq!(pcm.len(), 48_000);
    let f = freq(&pcm[3200..]);
    assert!((f - 440.0).abs() < 10.0, "got {f} Hz");
    let kbps = fs::metadata(&p).unwrap().len() * 8 / 3 / 1000;
    assert!(kbps < 40, "{kbps} kbps");
}

#[test]
fn waveform_is_64_normalised_bars() {
    let mut pcm = sine(300.0, 1000, 3000.0);
    pcm.extend(sine(300.0, 1000, 12000.0));
    pcm.extend(vec![0; 16_000]);
    let info = record(&tmp("wf.opus"), &pcm);
    assert_eq!(info.waveform.len(), 64);
    assert_eq!(*info.waveform.iter().max().unwrap(), 255);
    assert!(info.waveform[0] < 100 && info.waveform[30] > 200 && info.waveform[63] == 0);
    // Very short and silent recordings still give 64 bars.
    let short = record(&tmp("short.opus"), &sine(300.0, 100, 5000.0));
    assert_eq!(short.waveform.len(), 64);
    assert_eq!(*short.waveform.iter().max().unwrap(), 255);
    let silent = record(&tmp("silent.opus"), &vec![0; 8000]);
    assert_eq!(silent.waveform, vec![0u8; 64]);
}

#[test]
fn cancel_removes_file() {
    let p = tmp("cancel.opus");
    let r = VoiceRecorder::start(p.clone()).unwrap();
    r.push(sine(300.0, 500, 5000.0)).unwrap();
    assert!(Path::new(&p).exists());
    r.cancel();
    assert!(!Path::new(&p).exists());
    assert!(r.push(vec![0; 10]).is_err());
}

#[test]
fn seek_lands_on_the_right_sample() {
    let p = tmp("seek.opus");
    let mut pcm = sine(300.0, 2000, 10000.0);
    pcm.extend(sine(900.0, 2000, 10000.0));
    record(&p, &pcm);
    let d = VoiceDecoder::open(p).unwrap();
    d.seek(3000);
    assert_eq!(d.position_ms(), 3000);
    let got = d.read(8000).unwrap();
    let f = freq(&got);
    assert!((f - 900.0).abs() < 20.0, "got {f} Hz");
    d.seek(0);
    assert!((freq(&d.read(8000).unwrap()[800..]) - 300.0).abs() < 10.0);
    d.seek(999_999);
    assert!(d.read(100).unwrap().is_empty());
}

// ---- hostile-input tests -------------------------------------------------------------

fn head_pkt() -> Vec<u8> {
    let mut h = b"OpusHead".to_vec();
    h.extend_from_slice(&[1, 1]);
    h.extend_from_slice(&312u16.to_le_bytes());
    h.extend_from_slice(&VOICE_RATE.to_le_bytes());
    h.extend_from_slice(&0i16.to_le_bytes());
    h.push(0);
    h
}

fn tags_pkt() -> Vec<u8> {
    let mut t = b"OpusTags".to_vec();
    t.extend_from_slice(&0u32.to_le_bytes());
    t.extend_from_slice(&0u32.to_le_bytes());
    t
}

fn silence_pkt() -> Vec<u8> {
    let mut e = Encoder::new(&CODEC).unwrap();
    encode_packet(&mut e, &[0; FRAME]).unwrap()
}

/// head, tags, then `n` single-packet audio pages (last is EOS), all valid.
fn build(head: Vec<u8>, tags: Vec<u8>, audio: &[Vec<u8>]) -> Vec<u8> {
    let mut f = page(0x02, 0, 0, &[head]);
    f.extend(page(0, 0, 1, &[tags]));
    for (i, a) in audio.iter().enumerate() {
        let last = i + 1 == audio.len();
        f.extend(page(if last { 0x04 } else { 0 }, (i as u64 + 1) * 960 + 312, 2 + i as u32, std::slice::from_ref(a)));
    }
    f
}

fn good() -> Vec<u8> {
    build(head_pkt(), tags_pkt(), &vec![silence_pkt(); 4])
}

/// Rejected at the Ogg layer (and therefore by the decoder).
#[track_caller]
fn rejects_ogg(f: &[u8]) {
    assert!(read_ogg(f).is_err(), "accepted a bad file");
    rejects(f);
}

/// Rejected by `VoiceDecoder::open`.
#[track_caller]
fn rejects(f: &[u8]) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let p = tmp(&format!("bad{}.opus", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    fs::write(&p, f).unwrap();
    assert!(VoiceDecoder::open(p).is_err());
}

#[test]
fn handmade_good_file_opens() {
    let p = tmp("good.opus");
    fs::write(&p, good()).unwrap();
    assert!(VoiceDecoder::open(p).is_ok());
}

#[test]
fn truncation_always_fails() {
    let f = record_bytes();
    for cut in 0..f.len() {
        assert!(read_ogg(&f[..cut]).is_err(), "cut {cut}");
    }
    let mut s = 12345u64;
    for _ in 0..50 {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let cut = (s >> 33) as usize % f.len();
        rejects_ogg(&f[..cut]);
    }
}

fn record_bytes() -> Vec<u8> {
    let p = tmp(&format!("bytes{:?}.opus", std::thread::current().id()));
    record(&p, &sine(440.0, 1500, 8000.0));
    fs::read(p).unwrap()
}

#[test]
fn any_flipped_byte_fails() {
    let f = good();
    for i in 0..f.len() {
        let mut g = f.clone();
        g[i] ^= 0x01;
        assert!(read_ogg(&g).is_err(), "flip at {i} accepted");
    }
    let f = record_bytes();
    for i in (0..f.len()).step_by(7) {
        let mut g = f.clone();
        g[i] ^= 0x80;
        assert!(read_ogg(&g).is_err(), "flip at {i} accepted");
    }
}

#[test]
fn framing_violations_rejected() {
    let a = silence_pkt();
    let h = head_pkt();
    let t = tags_pkt();
    let pg = |flags, gr, ser: u32, seq, pk: &[Vec<u8>]| {
        let mut v = page(flags, gr, seq, pk);
        v[14..18].copy_from_slice(&ser.to_le_bytes());
        let mut z = v.clone();
        z[22..26].copy_from_slice(&[0; 4]);
        let c = crc(&z);
        v[22..26].copy_from_slice(&c.to_le_bytes());
        v
    };
    let s = SERIAL;
    let ok = |extra: Vec<u8>| {
        let mut f = pg(0x02, 0, s, 0, std::slice::from_ref(&h));
        f.extend(pg(0, 0, s, 1, std::slice::from_ref(&t)));
        f.extend(extra);
        f
    };
    rejects_ogg(&ok(pg(0x04, 960, s + 1, 2, std::slice::from_ref(&a)))); // wrong serial
    rejects_ogg(&ok(pg(0x04, 960, s, 3, std::slice::from_ref(&a)))); // skipped sequence
    rejects_ogg(&ok([pg(0, 960, s, 2, std::slice::from_ref(&a)), pg(0x06, 1920, s, 3, std::slice::from_ref(&a))].concat())); // BOS twice
    rejects_ogg(&ok(pg(0x05, 960, s, 2, std::slice::from_ref(&a)))); // continued flag with nothing open
    rejects_ogg(&ok(pg(0x04, 960, s, 2, &[vec![1u8; MAX_AUDIO_PACKET_BYTES + 1]]))); // oversized packet
    rejects_ogg(&ok(pg(0x08, 960, s, 2, std::slice::from_ref(&a)))); // unknown flag
    rejects_ogg(&ok([pg(0, 1920, s, 2, std::slice::from_ref(&a)), pg(0x04, 960, s, 3, std::slice::from_ref(&a))].concat())); // granule backwards
    rejects_ogg(&ok([pg(0x04, 960, s, 2, std::slice::from_ref(&a)), pg(0x04, 1920, s, 3, std::slice::from_ref(&a))].concat())); // after EOS
    rejects_ogg(&ok(pg(0, 960, s, 2, std::slice::from_ref(&a)))); // no EOS
    rejects_ogg(&ok(pg(0x04, 1 << 60, s, 2, std::slice::from_ref(&a)))); // absurd duration
    // Open (255-lacing) packet left dangling at EOS, and a missing continuation.
    rejects_ogg(&ok(raw_page(0x04, 960, 2, &[255], &[0u8; 255])));
    // Version byte.
    let mut v = good();
    v[4] = 1;
    rejects_ogg(&v);
    // Empty audio packet.
    rejects(&build(h.clone(), t.clone(), &[vec![]]));
    // No audio at all.
    let mut f = pg(0x02, 0, s, 0, std::slice::from_ref(&h));
    f.extend(pg(0x04, 0, s, 1, std::slice::from_ref(&t)));
    rejects_ogg(&f);
    // A single BOS|EOS page holding only OpusHead (used to panic in open).
    rejects_ogg(&pg(0x06, 0, s, 0, std::slice::from_ref(&h)));
}

/// A page with explicit lacing, correct CRC.
fn raw_page(flags: u8, granule: u64, seq: u32, lacing: &[u8], body: &[u8]) -> Vec<u8> {
    let mut v = b"OggS".to_vec();
    v.extend_from_slice(&[0, flags]);
    v.extend_from_slice(&granule.to_le_bytes());
    v.extend_from_slice(&SERIAL.to_le_bytes());
    v.extend_from_slice(&seq.to_le_bytes());
    v.extend_from_slice(&[0; 4]);
    v.push(lacing.len() as u8);
    v.extend_from_slice(lacing);
    v.extend_from_slice(body);
    let c = crc(&v);
    v[22..26].copy_from_slice(&c.to_le_bytes());
    v
}

#[test]
fn continuation_across_pages_is_accepted_and_checked() {
    // A 300-byte tags packet: 255 bytes on one page (open), 45 on the next (continued).
    let mut t = tags_pkt();
    t.resize(300, 0);
    let build = |second_flags: u8, first_lacing: &[u8]| {
        let mut f = page(0x02, 0, 0, &[head_pkt()]);
        f.extend(raw_page(0, u64::MAX, 1, first_lacing, &t[..255]));
        f.extend(raw_page(second_flags, 0, 2, &[45], &t[255..]));
        f.extend(page(0x04, 960 + 312, 3, &[silence_pkt()]));
        f
    };
    assert!(read_ogg(&build(0x01, &[255])).is_ok());
    rejects_ogg(&build(0x00, &[255])); // open packet but next page not marked continued
    rejects_ogg(&build(0x01, &[254])); // marked continued but previous page closed its packet
}

#[test]
fn bad_opus_head_and_tags_rejected() {
    let a = vec![silence_pkt(); 2];
    let mutate = |i: usize, v: u8| {
        let mut h = head_pkt();
        h[i] = v;
        build(h, tags_pkt(), &a)
    };
    rejects(&mutate(0, b'X')); // magic
    rejects(&mutate(8, 0x10)); // major version
    rejects(&mutate(9, 2)); // stereo
    rejects(&mutate(9, 0)); // zero channels
    rejects(&mutate(18, 1)); // mapping family
    let mut h = head_pkt();
    h[10..12].copy_from_slice(&60000u16.to_le_bytes());
    rejects(&build(h, tags_pkt(), &a)); // preskip
    let mut h = head_pkt();
    h.push(0);
    rejects(&build(h, tags_pkt(), &a)); // length
    let mut t = tags_pkt();
    t[0] = b'X';
    rejects(&build(head_pkt(), t, &a)); // OpusTags magic
    rejects(&build(head_pkt(), vec![b'O'; MAX_TAGS_BYTES + 1], &a)); // tags too big
    // Audio packet that is not Opus-decodable length-wise is bounded by the sample cap.
    let mut v = good();
    v.truncate(v.len() - 1);
    rejects(&v);
}

#[test]
fn size_and_count_limits() {
    rejects_ogg(&vec![0u8; MAX_VOICE_FILE as usize + 1]);
    let n = MAX_AUDIO_PACKETS + 1;
    let mut f = page(0x02, 0, 0, &[head_pkt()]);
    f.extend(page(0, 0, 1, &[tags_pkt()]));
    let a = silence_pkt();
    let mut seq = 2;
    let mut left = n;
    while left > 0 {
        let k = left.min(40);
        left -= k;
        let flags = if left == 0 { 0x04 } else { 0 };
        f.extend(page(flags, 960, seq, &vec![a.clone(); k]));
        seq += 1;
    }
    assert!(f.len() as u64 <= MAX_VOICE_FILE, "test file must hit the packet cap, not the size cap");
    assert!(read_ogg(&f).unwrap_err().to_string().contains("too many packets"));
}

#[test]
fn random_mutations_never_panic() {
    let seeds = [good(), record_bytes()];
    let mut s = 0x1234_5678_9abc_def0u64;
    let mut next = move || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (s >> 33) as usize
    };
    let p = tmp("fuzz.opus");
    for it in 0..2000 {
        let mut f = seeds[it % 2].clone();
        for _ in 0..1 + next() % 4 {
            match next() % 4 {
                0 => {
                    let i = next() % f.len();
                    f[i] = next() as u8;
                }
                1 => f.truncate(1 + next() % f.len()),
                2 => {
                    let i = next() % f.len();
                    f.insert(i, next() as u8);
                }
                _ => {
                    // Fix up the CRC of a random page so the structural checks run past it.
                    if let Some(o) = (0..f.len().saturating_sub(27)).filter(|&o| &f[o..o + 4] == b"OggS").nth(next() % 3) {
                        let i = o + 4 + next() % 22;
                        f[i] = next() as u8;
                        if let Some(h) = f.get(o..o + 27) {
                            let n = h[26] as usize;
                            if let Some(lac) = f.get(o + 27..o + 27 + n) {
                                let end = o + 27 + n + lac.iter().map(|&b| b as usize).sum::<usize>();
                                if end <= f.len() {
                                    f[o + 22..o + 26].copy_from_slice(&[0; 4]);
                                    let c = crc(&f[o..end]);
                                    f[o + 22..o + 26].copy_from_slice(&c.to_le_bytes());
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Ok(o) = read_ogg(&f) {
            assert!(o.packets.len() <= MAX_AUDIO_PACKETS + 2);
            assert!(o.packets.iter().all(|p| p.len() <= MAX_AUDIO_PACKET_BYTES.max(MAX_TAGS_BYTES)));
        }
        fs::write(&p, &f).unwrap();
        if let Ok(d) = VoiceDecoder::open(p.clone()) {
            for _ in 0..5 {
                if d.read(4000).unwrap_or_default().is_empty() {
                    break;
                }
            }
            d.seek(next() as u32 % 10_000);
            let _ = d.read(100);
        }
    }
}

/// Both ffprobe (container, codec, rate, duration) and opusinfo (page CRCs) when installed.
#[test]
fn file_is_valid_ogg_opus() {
    let p = tmp("valid.opus");
    record(&p, &sine(500.0, 2500, 9000.0));
    let Ok(out) = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_name,sample_rate,channels:format=format_name,duration", "-of", "default=nw=1", &p])
        .output()
    else {
        eprintln!("ffprobe not installed; skipping");
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("codec_name=opus") && text.contains("format_name=ogg") && text.contains("channels=1"), "{text}");
    let dur: f32 = text.lines().find_map(|l| l.strip_prefix("duration=")).unwrap().parse().unwrap();
    assert!((dur - 2.5).abs() < 0.05, "{text}");
    // Decode the whole thing with ffmpeg's libopus-independent path: any CRC or sequence error is reported.
    let dec = std::process::Command::new("ffmpeg").args(["-v", "error", "-i", &p, "-f", "null", "-"]).output();
    if let Ok(d) = dec {
        assert!(d.stderr.is_empty(), "{}", String::from_utf8_lossy(&d.stderr));
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Byte-identical regression for the voice-message encoder/decoder settings (hashes recorded
/// before the move onto `audio::codec`).
#[test]
fn voice_file_and_decode_golden() {
    let p = tmp("golden.opus");
    let mut s = 12345u64;
    let pcm: Vec<i16> = (0..16 * 1500)
        .map(|i| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let t = i as f64 / 16_000.0;
            (7000.0 * (std::f64::consts::TAU * 300.0 * t).sin() + ((s >> 40) as i64 % 600 - 300) as f64) as i16
        })
        .collect();
    let info = record(&p, &pcm);
    let file = fs::read(&p).unwrap();
    let dec: Vec<u8> = read_all(&p).iter().flat_map(|s| s.to_le_bytes()).collect();
    assert_eq!(info.duration_ms, 1500);
    assert_eq!((fnv(&file), fnv(&dec)), (GOLDEN_FILE, GOLDEN_DECODE), "file {:#x} decode {:#x}", fnv(&file), fnv(&dec));
}

const GOLDEN_FILE: u64 = 0x4665e0135bf1101e;
const GOLDEN_DECODE: u64 = 0xc4330d6733b83f46;
