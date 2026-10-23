//! Live controls for the current call (the in-call "stats for nerds" sheet, developer builds):
//! send codec, test size and bitrate cap, applied to the running sender. Settings > Diagnostics
//! tuning (`tuning`) stays the next-call default; this overrides it for this call only.
//!
//! - Size and cap are ours alone: `Sender::set_test_limits` on the running sender.
//! - VP8 hardware <-> VP8 software is the same bitstream: a new encoder and a keyframe.
//! - VP8 <-> AV1 changes what the peer decodes: `run_call` first sends `Msg::VideoCodec`
//!   ("my video is X from my next keyframe"), then the send thread swaps encoders; the peer's
//!   decoder swaps at the first keyframe of X. Only to a peer whose offer says `codec_switch`
//!   and that decodes X (`docs/video.md`, "Switching codecs mid-call").
//!
//! - The decoder (Auto / hardware / software, where this device has both for the arriving codec)
//!   is mine alone: the decode thread swaps at the next keyframe and asks the sender for one
//!   through the existing keyframe request (video control message 2). No protocol change.
//!
//! Nothing here turns a camera on: without my camera nothing is encoded, whatever is chosen.
use super::*;
use super::codecs::{EncoderChoice, decoder_kinds, decoder_name, local_encoders, local_offer_codecs};

/// "" and "auto" are Auto.
pub(super) fn decoder_id(id: &str) -> &str {
    if id.is_empty() { "auto" } else { id }
}

/// A decoder id as the decode thread's preference: None Auto, Some(true) hardware.
pub(super) fn decoder_pref(id: &str) -> Option<bool> {
    match id {
        "hw" => Some(true),
        "sw" => Some(false),
        _ => None,
    }
}

/// The decoder choices for `codec` arriving now: Auto, then hardware and software only where
/// this device has both (AV1 here is software only, so it gets Auto alone, labelled so).
pub(super) fn decoder_choices(codec: VideoCodec) -> Vec<VideoChoice> {
    let k = decoder_kinds(codec);
    let name = |hw| format!("{}, {}", video_call_core::adapt::codec_label(codec.as_str().into(), hw), decoder_name(codec, hw));
    let note = if k.hardware && k.software {
        String::new()
    } else {
        format!("The only {} decoder on this device", codec.as_str().to_uppercase())
    };
    let mut v = vec![VideoChoice { id: "auto".into(), label: format!("Auto ({})", name(k.auto_hardware)), available: true, why: note }];
    if k.hardware && k.software {
        for (id, hw) in [("hw", true), ("sw", false)] {
            v.push(VideoChoice { id: id.into(), label: name(hw), available: true, why: String::new() });
        }
    }
    v
}

/// One send-codec choice for the sheet. `available` false = shown greyed with `why`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoChoice {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub why: String,
}

fn choice_label(c: EncoderChoice) -> String {
    video_call_core::adapt::codec_label(c.codec.as_str().into(), c.hardware)
}

/// Whether `want` can be used in this call: `Err(why)` for the sheet and `set_video_live`.
/// `current` is the codec the peer decodes now.
pub(super) fn check(want: EncoderChoice, current: VideoCodec, peer: &VideoOffer, local: &[EncoderChoice]) -> Result<(), String> {
    if !local.contains(&want) {
        return Err("This device has no such encoder".into());
    }
    if !peer.decode.contains(&want.codec) {
        return Err(format!("Their device doesn’t decode {}", want.codec.as_str().to_uppercase()));
    }
    if want.codec != current && !peer.codec_switch {
        return Err("Their Tinline can’t switch codecs during a call. It applies from your next call".into());
    }
    Ok(())
}

/// The sender config for this call's live size, fps and cap: the pin bounded by what the peer's
/// offer takes (its decoder refuses more), whatever `ignore_limits` says.
pub(super) fn sender_config(pipe: &Pipe) -> SenderConfig {
    let mut cfg = SenderConfig::default();
    let (live, tuning) = {
        let st = pipe.stats.lock();
        (st.live.clone(), st.tuning.clone())
    };
    live.limits().configure(&mut cfg);
    tuning.configure_rate(&mut cfg, pipe.policy.lock().peer_probes);
    let (offer_rung, offer_fps) = pipe.policy.lock().peer;
    let l = &mut cfg.ladder;
    l.fixed_rung = l.fixed_rung.map(|r| r.min(offer_rung));
    l.fixed_fps = (live.fixed_fps != 0).then(|| live.fixed_fps.min(offer_fps));
    l.ignore_caps = live.ignore_limits && l.fixed_rung.is_some();
    cfg
}

/// Pushes this call's size, fps and cap into the running sender (no-op before it runs).
pub(super) fn apply_limits(pipe: &Pipe) {
    let Some(send) = pipe.send.lock().clone() else { return };
    let cfg = sender_config(pipe);
    send.with_sender(|s| s.set_test_limits(&cfg));
}

impl CallVideo {
    /// The peer's `Msg::VideoCodec`: its video is `codec` from its next keyframe.
    pub fn remote_codec(&self, codec: VideoCodec) {
        if !self.negotiated() {
            return;
        }
        if !local_offer_codecs().decode.contains(&codec) {
            tracing::info!("video peer codec {}: not decoded here, ignored", codec.as_str());
            return;
        }
        tracing::info!("video peer codec -> {} (from its next keyframe)", codec.as_str());
        *self.pipe.recv_switch.lock() = Some(codec);
    }

    /// The call's live overrides (`None` without video).
    pub fn live(&self) -> Option<VideoLive> {
        self.negotiated().then(|| self.pipe.stats.lock().live.clone())
    }

    /// The send codecs for this call: Auto (what the call negotiated) and each encoder this device
    /// has, with whether the peer takes it now. Empty without video.
    pub fn live_codecs(&self) -> Vec<VideoChoice> {
        let (Some(peer), Some(auto)) = (self.peer_video.lock().clone(), *self.auto_encoder.lock()) else { return Vec::new() };
        let current = self.pipe.encoder.lock().map_or(auto.codec, |c| c.codec);
        let local = local_encoders();
        let mut out = vec![VideoChoice { id: "auto".into(), label: format!("Auto ({})", choice_label(auto)), available: true, why: String::new() }];
        for c in &local {
            let ok = check(*c, current, &peer, &local);
            out.push(VideoChoice { id: c.id().into(), label: choice_label(*c), available: ok.is_ok(), why: ok.err().unwrap_or_default() });
        }
        out
    }

    /// The decoder choices for the codec arriving now (see `VideoLive::recv_decoder`). Empty
    /// without video.
    pub fn live_decoders(&self) -> Vec<VideoChoice> {
        if !self.negotiated() {
            return Vec::new();
        }
        let codec = match self.pipe.stats.lock().recv_codec.as_str() {
            "vp8" => VideoCodec::Vp8,
            _ => VideoCodec::Av1,
        };
        decoder_choices(codec)
    }

    /// Applies `live` at once: the size and cap to the running sender, a new encoder (and, for
    /// another codec, `Msg::VideoCodec` to the peer first, by `run_call`). Errors leave the call as
    /// it was. `NotFound` without negotiated peer state, `Protocol` without video or for a choice
    /// the call can't take (the message says why).
    pub fn set_live(&self, live: VideoLive) -> Result<(), Error> {
        let v = self;
        if !v.negotiated() {
            return Err(Error::Protocol("no video in this call".into()));
        }
        live.limits().validate(&local_offer_codecs()).map_err(|e| Error::Protocol(e.to_string()))?;
        let peer = v.peer_video.lock().clone().ok_or(Error::NotFound)?;
        let auto = (*v.auto_encoder.lock()).ok_or(Error::NotFound)?;
        let want = match live.send_codec.as_str() {
            "" | "auto" => auto,
            id => EncoderChoice::from_id(id).ok_or_else(|| Error::Protocol("unknown send codec".into()))?,
        };
        let current = v.pipe.encoder.lock().map_or(auto.codec, |c| c.codec);
        if want != auto {
            check(want, current, &peer, &local_encoders()).map_err(Error::Protocol)?;
        }
        if !matches!(live.recv_decoder.as_str(), "" | "auto" | "hw" | "sw") {
            return Err(Error::Protocol("unknown decoder".into()));
        }
        if !matches!(live.fixed_fps, 0 | 15 | 24 | 30) {
            return Err(Error::Protocol("unsupported frame rate".into()));
        }
        if live.ignore_limits && live.fixed_height == 0 {
            return Err(Error::Protocol("Pin a size first: Ignore limits holds a pinned size".into()));
        }
        let live = VideoLive {
            send_codec: if want == auto { "auto".into() } else { want.id().into() },
            recv_decoder: decoder_id(&live.recv_decoder).into(),
            ..live
        };
        let before = std::mem::replace(&mut v.pipe.stats.lock().live, live.clone());
        tracing::info!(
            "video live: codec {} size {} fps {} cap {} ignore_limits={} decoder {} (was {before:?})",
            live.send_codec, live.fixed_height, live.fixed_fps, live.max_bps, live.ignore_limits, live.recv_decoder
        );
        apply_limits(&v.pipe);
        *v.pipe.decoder_pref.lock() = decoder_pref(&live.recv_decoder);
        v.encoder_req.send_if_modified(|r| std::mem::replace(r, Some(want)) != Some(want));
        Ok(())
    }
}

#[cfg(test)]
mod tests;
