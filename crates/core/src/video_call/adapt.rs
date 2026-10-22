//! The adaptation runtime is `call_video::adapt`; the policy itself (drawn size, budget, decode
//! cost, labels) is `video_call_core::adapt`. This declares its records to UniFFI and exports the
//! picture labels.

use super::*;

/// What the platform tells us about the device (`Node::set_video_budget`).
#[uniffi::remote(Record)]
pub struct VideoBudget {
    /// Android `PowerManager` thermal status (0 none, 1 light, 2 moderate, 3 severe, ...);
    /// 0 where there is no such thing.
    pub thermal: u8,
    /// Battery saver is on.
    pub power_save: bool,
}

/// What caps our sending, as `VideoStats.send_cap` reports it.
#[uniffi::remote(Record)]
pub struct VideoSendCap {
    /// Largest picture (pixels) we may send now: the top rung when nothing caps it.
    pub pixels: u32,
    /// Highest frame rate we may send now.
    pub fps: u32,
    /// Why, for the chip: "none", "display", "decode", "thermal", "battery" or "user".
    pub reason: String,
    /// Whose limit it is: "peer" (their hint: their screen, decoder, heat, battery saver),
    /// "device" (my own budget, or their offer's largest size), "" when nothing caps it.
    pub source: String,
}

/// The picture label for what we send, from the stats (one text for every app): "HD · 24 fps"
/// while the top size is sent; below it, why ("540p · slow network", "540p · their screen"), else
/// the rate. Empty before a frame was sent.
#[uniffi::export]
pub fn video_send_label(stats: &VideoStats) -> String {
    video_call_core::adapt::send_label(stats)
}

/// The picture label for what I receive ("Their video \u{b7} 540p \u{b7} 24 fps"); empty before
/// the first decoded frame. Both apps show it on the remote picture.
#[uniffi::export]
pub fn video_recv_label(stats: &VideoStats) -> String {
    video_call_core::adapt::recv_label(stats)
}

/// The codec of one direction for call details, from `VideoStats.{send,recv}_codec` and
/// `{send,recv}_hardware`: "VP8 \u{b7} hardware", "AV1 \u{b7} software". Empty before the codec is decided.
#[uniffi::export]
pub fn video_codec_label(codec: String, hardware: bool) -> String {
    video_call_core::adapt::codec_label(codec, hardware)
}
