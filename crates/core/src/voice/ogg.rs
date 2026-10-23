//! Ogg framing: page writer, CRC and the validating reader (every limit is checked before
//! anything is allocated).

use super::*;

// ---------------------------------------------------------------------------------------------
// Ogg framing
// ---------------------------------------------------------------------------------------------

pub(super) fn crc_table() -> &'static [u32; 256] {
    pub(super) static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 { (r << 1) ^ 0x04c1_1db7 } else { r << 1 };
            }
            *e = r;
        }
        t
    })
}

pub(super) fn crc(data: &[u8]) -> u32 {
    crc_update(0, data)
}

/// One Ogg page holding whole packets (a packet's lacing may span several segments).
pub(super) fn page(flags: u8, granule: u64, seq: u32, packets: &[Vec<u8>]) -> Vec<u8> {
    let mut lacing = Vec::new();
    for p in packets {
        lacing.extend(std::iter::repeat_n(255u8, p.len() / 255));
        lacing.push((p.len() % 255) as u8);
    }
    let mut out = Vec::with_capacity(27 + lacing.len() + packets.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(b"OggS");
    out.push(0);
    out.push(flags);
    out.extend_from_slice(&granule.to_le_bytes());
    out.extend_from_slice(&SERIAL.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.push(lacing.len() as u8);
    out.extend_from_slice(&lacing);
    for p in packets {
        out.extend_from_slice(p);
    }
    let c = crc(&out);
    out[22..26].copy_from_slice(&c.to_le_bytes());
    out
}

/// Hard limits applied to a voice file before anything is allocated for it. The recorder has no
/// duration cap of its own; playback accepts up to 30 minutes (24 kbps VBR is about 5.4 MB for
/// that), and 16 MiB leaves a generous margin for louder/noisier audio and page overhead.
pub const MAX_VOICE_FILE: u64 = 16 * 1024 * 1024;
/// 30 minutes of 20 ms packets is 90 000; round up.
pub(super) const MAX_AUDIO_PACKETS: usize = 100_000;
/// RFC 6716: a packet holds at most 120 ms, i.e. six 20 ms frames of at most 1275 bytes each.
pub(super) const MAX_AUDIO_PACKET_BYTES: usize = MAX_PACKET * 6;
pub(super) const MAX_HEAD_BYTES: usize = 19;
pub(super) const MAX_TAGS_BYTES: usize = 64 * 1024;
pub(super) const MAX_DURATION_SAMPLES_48K: u64 = 30 * 60 * 48_000;
pub(super) const MAX_PRESKIP: u16 = 9_600;
/// Samples (16 kHz) one packet may decode to: 120 ms.
pub(super) const MAX_PACKET_SAMPLES: i32 = 1920;

#[derive(Debug)]
pub(super) struct Ogg {
    pub(super) packets: Vec<Vec<u8>>,
    /// Granule position of the final (EOS) page, in 48 kHz samples.
    pub(super) granule: u64,
}

pub(super) fn bad_ogg(m: &str) -> Error {
    Error::Audio(format!("not a valid Ogg Opus file: {m}"))
}

pub(super) fn crc_update(c: u32, data: &[u8]) -> u32 {
    let t = crc_table();
    data.iter().fold(c, |c, &b| (c << 8) ^ t[((c >> 24) as u8 ^ b) as usize])
}

/// Parses and validates a single-stream Ogg file, enforcing every limit before it allocates.
/// Packet 0 must be a bare OpusHead page, the last page must carry EOS and a real granule.
pub(super) fn read_ogg(data: &[u8]) -> Result<Ogg, Error> {
    if data.len() as u64 > MAX_VOICE_FILE {
        return Err(bad_ogg("file too large"));
    }
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut open = false; // previous page ended inside a packet
    let mut serial = 0u32;
    let mut next_seq = 0u32;
    let mut last_granule = 0u64;
    let mut eos = false;
    let mut pos = 0;
    while pos < data.len() {
        if eos {
            return Err(bad_ogg("data after end of stream"));
        }
        let h = data.get(pos..pos + 27).ok_or_else(|| bad_ogg("truncated header"))?;
        if &h[..4] != b"OggS" {
            return Err(bad_ogg("missing capture pattern"));
        }
        if h[4] != 0 {
            return Err(bad_ogg("unsupported version"));
        }
        let flags = h[5];
        if flags & !0x07 != 0 {
            return Err(bad_ogg("bad header flags"));
        }
        let first = pos == 0;
        if (flags & 0x02 != 0) != first {
            return Err(bad_ogg("BOS must be on the first page only"));
        }
        if (flags & 0x01 != 0) != open {
            return Err(bad_ogg("continuation flag does not match the previous page"));
        }
        let (Some(granule), Some(ser), Some(seq), Some(want_crc)) =
            (h[6..].first_chunk::<8>(), h[14..].first_chunk::<4>(), h[18..].first_chunk::<4>(), h[22..].first_chunk::<4>())
        else {
            return Err(bad_ogg("short page header"));
        };
        let (granule, ser, seq, want_crc) = (u64::from_le_bytes(*granule), u32::from_le_bytes(*ser), u32::from_le_bytes(*seq), u32::from_le_bytes(*want_crc));
        if first {
            serial = ser;
        } else if ser != serial {
            return Err(bad_ogg("more than one logical stream"));
        }
        if seq != next_seq {
            return Err(bad_ogg("page sequence gap"));
        }
        next_seq = next_seq.wrapping_add(1);
        let n = h[26] as usize;
        if n == 0 {
            return Err(bad_ogg("empty segment table"));
        }
        let lacing = data.get(pos + 27..pos + 27 + n).ok_or_else(|| bad_ogg("truncated lacing"))?;
        let body_len: usize = lacing.iter().map(|&b| b as usize).sum();
        let body = data.get(pos + 27 + n..pos + 27 + n + body_len).ok_or_else(|| bad_ogg("truncated page"))?;
        let mut c = crc_update(0, &h[..22]);
        c = crc_update(c, &[0; 4]);
        c = crc_update(c, &h[26..27]);
        c = crc_update(c, lacing);
        c = crc_update(c, body);
        if c != want_crc {
            return Err(bad_ogg("page checksum mismatch"));
        }
        if granule != u64::MAX {
            if granule >= 1 << 63 || granule < last_granule {
                return Err(bad_ogg("granule position out of range"));
            }
            if granule > MAX_DURATION_SAMPLES_48K + MAX_PRESKIP as u64 {
                return Err(bad_ogg("recording too long"));
            }
            last_granule = granule;
        }
        if flags & 0x04 != 0 {
            if granule == u64::MAX {
                return Err(bad_ogg("final page has no granule position"));
            }
            eos = true;
        }
        let mut off = 0;
        for &l in lacing {
            let limit = match packets.len() {
                0 => MAX_HEAD_BYTES,
                1 => MAX_TAGS_BYTES,
                _ => MAX_AUDIO_PACKET_BYTES,
            };
            if cur.len() + l as usize > limit {
                return Err(bad_ogg("packet too large"));
            }
            cur.extend_from_slice(&body[off..off + l as usize]);
            off += l as usize;
            open = l == 255;
            if !open {
                if packets.len() >= MAX_AUDIO_PACKETS + 2 {
                    return Err(bad_ogg("too many packets"));
                }
                packets.push(std::mem::take(&mut cur));
            }
        }
        if first && (open || packets.len() != 1) {
            return Err(bad_ogg("OpusHead must be alone on the first page"));
        }
        pos += 27 + n + body_len;
    }
    if open {
        return Err(bad_ogg("unfinished final packet"));
    }
    if !eos {
        return Err(bad_ogg("missing end of stream (truncated file)"));
    }
    // OpusHead, OpusTags and at least one audio packet; callers index the first two.
    if packets.len() < 3 {
        return Err(bad_ogg("no audio"));
    }
    Ok(Ogg { packets, granule: last_granule })
}
