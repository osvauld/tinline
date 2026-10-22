//! Which codecs this device offers, and the encoder/decoder for a negotiated direction
//! (`docs/design/video-hw-vp8.md`). AV1 everywhere (software); VP8 on the desktop through libvpx
//! (feature `vp8`, encode and decode, software) and on Android through MediaCodec (feature
//! `mediacodec`; encoding only with a hardware encoder). The offer says which are hardware.

use proto::VideoCodec;
use video::aom::AomEncoder;
use video::{Decoder, EncoderConfig, VideoDecoder, VideoEncoder};

use super::DECODER_THREADS;
#[cfg(all(target_os = "android", feature = "mediacodec"))]
use video::mediacodec;

pub(crate) use video_call_core::LocalCodecs;

pub(crate) fn local_offer_codecs() -> LocalCodecs {
    let mut c = LocalCodecs {
        decode: vec![VideoCodec::Av1],
        encode: vec![VideoCodec::Av1],
        hw_decode: vec![],
        hw_encode: vec![],
    };
    if vp8_decoder() {
        c.decode.push(VideoCodec::Vp8);
    }
    if hardware_vp8_decoder() {
        c.hw_decode.push(VideoCodec::Vp8);
    }
    if hardware_vp8_encoder() {
        // Which codec a direction uses is `proto::send_codec`'s call; the order here only breaks
        // ties, and a phone prefers its hardware codec.
        c.encode.insert(0, VideoCodec::Vp8);
        c.hw_encode.push(VideoCodec::Vp8);
    } else if software_vp8_encoder() {
        // Desktop libvpx: offered, but only chosen when the receiver decodes VP8 in hardware.
        c.encode.push(VideoCodec::Vp8);
    }
    c
}

fn vp8_decoder() -> bool {
    cfg!(any(
        feature = "vp8",
        all(target_os = "android", feature = "mediacodec")
    ))
}

/// libvpx encoder: desktop builds with feature `vp8`; Android never links it.
fn software_vp8_encoder() -> bool {
    cfg!(all(feature = "vp8", not(target_os = "android")))
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn hardware_vp8_encoder() -> bool {
    mediacodec::hardware_vp8_encoder()
}

#[cfg(not(all(target_os = "android", feature = "mediacodec")))]
fn hardware_vp8_encoder() -> bool {
    false
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn hardware_vp8_decoder() -> bool {
    mediacodec::hardware_vp8_decoder()
}

#[cfg(not(all(target_os = "android", feature = "mediacodec")))]
fn hardware_vp8_decoder() -> bool {
    false
}

/// Whether our decoder for `codec` runs through MediaCodec (Android VP8, hardware or not, e.g.
/// the moto g52's `c2.android.vp8.decoder`): asynchronous, so the wall time of a decode call is
/// pipeline latency and up to `DECODE_BUDGET` of waiting, not CPU work. The decode-cost policy
/// then judges it by throughput, as it does hardware decoders (`adapt::Policy::observe_decode_hw`).
pub(crate) fn pipelined_decoder(codec: VideoCodec) -> bool {
    codec == VideoCodec::Vp8 && cfg!(all(target_os = "android", feature = "mediacodec"))
}

/// The decoders this device has for one codec, and which one Auto (the offer's `hw_decode`)
/// uses. AV1 is rav1d (software) everywhere; VP8 is libvpx (software) on the desktop and
/// MediaCodec on Android: the platform's default (hardware where present) or Google's software one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DecoderKinds {
    pub hardware: bool,
    pub software: bool,
    pub auto_hardware: bool,
}

/// The decoder kinds of `codec` here, plus how to build one: in a table so the decode loop's
/// swap rules can be tested with stand-in decoders.
#[derive(Clone, Copy)]
pub(crate) struct Decoders {
    pub kinds: fn(VideoCodec) -> DecoderKinds,
    pub make: fn(VideoCodec, bool, u32) -> video::Result<Box<dyn VideoDecoder>>,
}

impl Decoders {
    pub(crate) const LOCAL: Self = Self { kinds: decoder_kinds, make: new_decoder };

    /// The decoder kind (`true` = hardware) for `codec` under the receiver's preference
    /// (`None` = Auto); a preference this device can't meet falls back to Auto's.
    pub(crate) fn pick(&self, codec: VideoCodec, pref: Option<bool>) -> bool {
        let k = (self.kinds)(codec);
        match pref {
            Some(true) if k.hardware => true,
            Some(false) if k.software => false,
            _ => k.auto_hardware,
        }
    }
}

pub(crate) fn decoder_kinds(codec: VideoCodec) -> DecoderKinds {
    match codec {
        VideoCodec::Av1 => DecoderKinds { hardware: false, software: true, auto_hardware: false },
        VideoCodec::Vp8 => {
            let hw = hardware_vp8_decoder();
            DecoderKinds { hardware: hw, software: vp8_decoder() && (!hw || android_software_vp8_decoder()), auto_hardware: hw }
        }
    }
}

/// The library behind a decoder or encoder, for the stats sheet.
pub(crate) fn decoder_name(codec: VideoCodec, hardware: bool) -> &'static str {
    match codec {
        VideoCodec::Av1 => "rav1d",
        VideoCodec::Vp8 if cfg!(target_os = "android") && hardware => "MediaCodec",
        VideoCodec::Vp8 if cfg!(target_os = "android") => "MediaCodec c2.android",
        VideoCodec::Vp8 => "libvpx",
    }
}

pub(crate) fn encoder_name(choice: EncoderChoice) -> &'static str {
    match choice.codec {
        VideoCodec::Av1 => "libaom",
        VideoCodec::Vp8 if cfg!(target_os = "android") && choice.hardware => "MediaCodec",
        VideoCodec::Vp8 if cfg!(target_os = "android") => "MediaCodec c2.android",
        VideoCodec::Vp8 => "libvpx",
    }
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn android_software_vp8_decoder() -> bool {
    mediacodec::software_vp8_decoder()
}

#[cfg(not(all(target_os = "android", feature = "mediacodec")))]
fn android_software_vp8_decoder() -> bool {
    false
}

/// The decoder for what the peer sends us (`hardware`: the hardware kind, see `DecoderKinds`).
/// `max_pixels` caps what the peer can make us allocate.
pub(crate) fn new_decoder(
    codec: VideoCodec,
    hardware: bool,
    max_pixels: u32,
) -> video::Result<Box<dyn VideoDecoder>> {
    match codec {
        VideoCodec::Av1 => Ok(Box::new(Decoder::new(DECODER_THREADS, max_pixels)?)),
        VideoCodec::Vp8 => new_vp8_decoder(hardware, max_pixels),
    }
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn new_vp8_decoder(hardware: bool, max_pixels: u32) -> video::Result<Box<dyn VideoDecoder>> {
    // The platform's default is the hardware decoder where there is one.
    Ok(Box::new(if hardware || !hardware_vp8_decoder() {
        mediacodec::MediaCodecVp8Decoder::new(max_pixels)?
    } else {
        mediacodec::MediaCodecVp8Decoder::new_software(max_pixels)?
    }))
}

#[cfg(all(
    feature = "vp8",
    not(all(target_os = "android", feature = "mediacodec"))
))]
fn new_vp8_decoder(_: bool, max_pixels: u32) -> video::Result<Box<dyn VideoDecoder>> {
    Ok(Box::new(video::vpx::Vp8Decoder::new(max_pixels)?))
}

#[cfg(not(any(feature = "vp8", all(target_os = "android", feature = "mediacodec"))))]
fn new_vp8_decoder(_: bool, _: u32) -> video::Result<Box<dyn VideoDecoder>> {
    Err(video::Error::Config("no VP8 decoder in this build".into()))
}

/// Which encoder our sender runs: a codec and whether it is the hardware one. VP8 hardware and
/// VP8 software make the same bitstream (the receiver can't tell); AV1 is always software.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EncoderChoice {
    pub codec: VideoCodec,
    pub hardware: bool,
}

impl EncoderChoice {
    /// The in-call sheet's id: "av1", "vp8-hw", "vp8-sw".
    pub fn id(self) -> &'static str {
        match (self.codec, self.hardware) {
            (VideoCodec::Av1, _) => "av1",
            (VideoCodec::Vp8, true) => "vp8-hw",
            (VideoCodec::Vp8, false) => "vp8-sw",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        let (codec, hardware) = match id {
            "av1" => (VideoCodec::Av1, false),
            "vp8-hw" => (VideoCodec::Vp8, true),
            "vp8-sw" => (VideoCodec::Vp8, false),
            _ => return None,
        };
        Some(Self { codec, hardware })
    }
}

/// Every encoder this device can run, for the in-call sheet (more than the offer lists: Android's
/// software VP8 encoder is never offered, so Auto's hardware score is unchanged).
pub(crate) fn local_encoders() -> Vec<EncoderChoice> {
    let mut v = vec![EncoderChoice { codec: VideoCodec::Av1, hardware: false }];
    if hardware_vp8_encoder() {
        v.push(EncoderChoice { codec: VideoCodec::Vp8, hardware: true });
    }
    if software_vp8_encoder() || android_software_vp8_encoder() {
        v.push(EncoderChoice { codec: VideoCodec::Vp8, hardware: false });
    }
    v
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn android_software_vp8_encoder() -> bool {
    mediacodec::software_vp8_encoder()
}

#[cfg(not(all(target_os = "android", feature = "mediacodec")))]
fn android_software_vp8_encoder() -> bool {
    false
}

/// Whether a keyframe's first bytes look like `codec`: a VP8 key frame's start code (bytes 3-5
/// `9d 01 2a`), or an AV1 temporal unit opening with a temporal delimiter or sequence header OBU.
/// Only steers *when* the receiver swaps decoders after `Msg::VideoCodec`; the decoder still
/// validates everything.
pub(crate) fn keyframe_is(codec: VideoCodec, data: &[u8]) -> bool {
    let vp8 = data.len() >= 10 && data[0] & 1 == 0 && data[3..6] == [0x9d, 0x01, 0x2a];
    match codec {
        VideoCodec::Vp8 => vp8,
        VideoCodec::Av1 => {
            !vp8 && data.first().is_some_and(|&b| b & 0x80 == 0 && matches!((b >> 3) & 0xf, 1 | 2))
        }
    }
}

/// The encoder for what we send the peer.
pub(crate) fn new_encoder(
    choice: EncoderChoice,
    cfg: &EncoderConfig,
) -> video::Result<Box<dyn VideoEncoder>> {
    match choice.codec {
        VideoCodec::Av1 => Ok(Box::new(AomEncoder::new(cfg)?)),
        VideoCodec::Vp8 => new_vp8_encoder(choice.hardware, cfg),
    }
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn new_vp8_encoder(hardware: bool, cfg: &EncoderConfig) -> video::Result<Box<dyn VideoEncoder>> {
    Ok(Box::new(if hardware {
        mediacodec::MediaCodecVp8Encoder::new(cfg)?
    } else {
        mediacodec::MediaCodecVp8Encoder::new_software(cfg)?
    }))
}

#[cfg(all(feature = "vp8", not(target_os = "android")))]
fn new_vp8_encoder(hardware: bool, cfg: &EncoderConfig) -> video::Result<Box<dyn VideoEncoder>> {
    if hardware {
        return Err(video::Error::Config("no hardware VP8 encoder here".into()));
    }
    Ok(Box::new(video::vpx::Vp8Encoder::new(cfg)?))
}

#[cfg(not(any(feature = "vp8", all(target_os = "android", feature = "mediacodec"))))]
fn new_vp8_encoder(_: bool, _: &EncoderConfig) -> video::Result<Box<dyn VideoEncoder>> {
    Err(video::Error::Config("no VP8 encoder in this build".into()))
}
