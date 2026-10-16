//! Which codecs this device offers, and the encoder/decoder for a negotiated direction
//! (`docs/design/video-hw-vp8.md`). AV1 everywhere; VP8 decoding on the desktop (libvpx,
//! feature `vp8`) and on Android (MediaCodec, feature `mediacodec`), VP8 encoding only on
//! Android with a hardware encoder.

use proto::VideoCodec;
use video::aom::AomEncoder;
use video::{Decoder, EncoderConfig, VideoDecoder, VideoEncoder};

use super::DECODER_THREADS;
#[cfg(all(target_os = "android", feature = "mediacodec"))]
use super::mediacodec;

/// This device's `VideoOffer` lists: (decode, encode), best first.
pub(crate) fn local_offer_codecs() -> (Vec<VideoCodec>, Vec<VideoCodec>) {
    let mut decode = vec![VideoCodec::Av1];
    let mut encode = vec![VideoCodec::Av1];
    if vp8_decoder() {
        decode.push(VideoCodec::Vp8);
    }
    if vp8_encoder() {
        // A hardware encoder beats software AV1 on the phones that have one.
        encode.insert(0, VideoCodec::Vp8);
    }
    (decode, encode)
}

fn vp8_decoder() -> bool {
    cfg!(any(
        feature = "vp8",
        all(target_os = "android", feature = "mediacodec")
    ))
}

#[cfg(all(target_os = "android", feature = "mediacodec"))]
fn vp8_encoder() -> bool {
    mediacodec::hardware_vp8_encoder()
}

#[cfg(not(all(target_os = "android", feature = "mediacodec")))]
fn vp8_encoder() -> bool {
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

#[cfg(not(all(target_os = "android", feature = "mediacodec")))]
fn new_vp8_encoder(_: &EncoderConfig) -> video::Result<Box<dyn VideoEncoder>> {
    Err(video::Error::Config("no VP8 encoder in this build".into()))
}
