//! Which codecs this device offers, and the encoder/decoder for a negotiated direction
//! (`docs/design/video-hw-vp8.md`). AV1 everywhere (software); VP8 on the desktop through libvpx
//! (feature `vp8`, encode and decode, software) and on Android through MediaCodec (feature
//! `mediacodec`; encoding only with a hardware encoder). The offer says which are hardware.

use proto::VideoCodec;
use video::aom::AomEncoder;
use video::{Decoder, EncoderConfig, VideoDecoder, VideoEncoder};

use super::DECODER_THREADS;
#[cfg(all(target_os = "android", feature = "mediacodec"))]
use super::mediacodec;

/// This device's `VideoOffer` codec lists, best first.
pub(crate) struct LocalCodecs {
    pub decode: Vec<VideoCodec>,
    pub encode: Vec<VideoCodec>,
    /// Subsets of `decode` / `encode` that run in hardware.
    pub hw_decode: Vec<VideoCodec>,
    pub hw_encode: Vec<VideoCodec>,
}

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

/// The decoder for what the peer sends us. `max_pixels` caps what the peer can make us allocate.
pub(crate) fn new_decoder(
    codec: VideoCodec,
    max_pixels: u32,
) -> video::Result<Box<dyn VideoDecoder>> {
    match codec {
        VideoCodec::Av1 => Ok(Box::new(Decoder::new(DECODER_THREADS, max_pixels)?)),
        VideoCodec::Vp8 => new_vp8_decoder(max_pixels),
    }
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn new_vp8_decoder(max_pixels: u32) -> video::Result<Box<dyn VideoDecoder>> {
    Ok(Box::new(mediacodec::MediaCodecVp8Decoder::new(max_pixels)?))
}

#[cfg(all(
    feature = "vp8",
    not(all(target_os = "android", feature = "mediacodec"))
))]
fn new_vp8_decoder(max_pixels: u32) -> video::Result<Box<dyn VideoDecoder>> {
    Ok(Box::new(video::vpx::Vp8Decoder::new(max_pixels)?))
}

#[cfg(not(any(feature = "vp8", all(target_os = "android", feature = "mediacodec"))))]
fn new_vp8_decoder(_: u32) -> video::Result<Box<dyn VideoDecoder>> {
    Err(video::Error::Config("no VP8 decoder in this build".into()))
}

/// The encoder for what we send the peer.
pub(crate) fn new_encoder(
    codec: VideoCodec,
    cfg: &EncoderConfig,
) -> video::Result<Box<dyn VideoEncoder>> {
    match codec {
        VideoCodec::Av1 => Ok(Box::new(AomEncoder::new(cfg)?)),
        VideoCodec::Vp8 => new_vp8_encoder(cfg),
    }
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn new_vp8_encoder(cfg: &EncoderConfig) -> video::Result<Box<dyn VideoEncoder>> {
    Ok(Box::new(mediacodec::MediaCodecVp8Encoder::new(cfg)?))
}

#[cfg(all(feature = "vp8", not(target_os = "android")))]
fn new_vp8_encoder(cfg: &EncoderConfig) -> video::Result<Box<dyn VideoEncoder>> {
    Ok(Box::new(video::vpx::Vp8Encoder::new(cfg)?))
}

#[cfg(not(any(feature = "vp8", all(target_os = "android", feature = "mediacodec"))))]
fn new_vp8_encoder(_: &EncoderConfig) -> video::Result<Box<dyn VideoEncoder>> {
    Err(video::Error::Config("no VP8 encoder in this build".into()))
}
