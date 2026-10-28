//! Live controls for the current call (the in-call "stats for nerds" sheet, developer builds):
//! send codec, test size and bitrate cap, applied to the running sender (`call_video::live`).
//! Settings > Diagnostics tuning (`tuning`) stays the next-call default; this overrides it for
//! this call only. This declares the records to UniFFI and exports the node's calls.
use super::*;
use crate::{Error, Node};

pub use call_video::VideoChoice;

/// This call's live overrides. `send_codec`: "auto" (what the call negotiated), "vp8-hw",
/// "vp8-sw" or "av1"; `max_bps` 0 or 0.5/1/2/3 Mbps; `fixed_height` 0, 360, 540 or 720 (the
/// pin); `fixed_fps` 0, 15, 24 or 30 (with a pinned size exactly that, else a ceiling);
/// `ignore_limits` (only with a pinned size): the pin holds against heat, battery saver and the
/// peer's hints, never above what the peer's offer takes; `recv_decoder` "auto", "hw" or "sw".
/// Per call, never saved: the next call starts from Settings again.
#[uniffi::remote(Record)]
pub struct VideoLive {
    pub send_codec: String,
    pub max_bps: u32,
    pub fixed_height: u32,
    pub fixed_fps: u32,
    pub ignore_limits: bool,
    pub recv_decoder: String,
}

/// One send-codec choice for the sheet. `available` false = shown greyed with `why`.
#[uniffi::remote(Record)]
pub struct VideoChoice {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub why: String,
}

#[uniffi::export]
impl Node {
    /// The current call's live overrides (`None` without a call with video).
    pub fn video_live(&self) -> Option<VideoLive> {
        self.open().current_video_call()?.video.live()
    }

    /// The send codecs for this call: Auto (what the call negotiated) and each encoder this device
    /// has, with whether the peer takes it now. Empty without a call with video.
    pub fn video_live_codecs(&self) -> Vec<VideoChoice> {
        self.open().current_video_call().map(|c| c.video.live_codecs()).unwrap_or_default()
    }

    /// The decoder choices for the codec arriving now (see `VideoLive::recv_decoder`). Empty
    /// without a call with video.
    pub fn video_live_decoders(&self) -> Vec<VideoChoice> {
        self.open().current_video_call().map(|c| c.video.live_decoders()).unwrap_or_default()
    }

    /// Applies `live` to the current call at once: the size and cap to the running sender, a new
    /// encoder (and, for another codec, `Msg::VideoCodec` to the peer first). Errors leave the
    /// call as it was. `NotFound` without a call, `Protocol` without video or for a choice the
    /// call can't take (the message says why).
    pub fn set_video_live(&self, live: VideoLive) -> Result<(), Error> {
        let call = self.open().current_video_call().ok_or(Error::NotFound)?;
        Ok(call.video.set_live(live)?)
    }
}
