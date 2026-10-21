//! Session-only developer controls. Apps hide these in production builds; defaults do nothing.
use proto::VideoCodec;
use video::media::SenderConfig;

use crate::Error;
use crate::nerds::stat_rows;
use crate::stats::VideoStats;

/// This device's `VideoOffer` codec lists, best first.
pub struct LocalCodecs {
    pub decode: Vec<VideoCodec>,
    pub encode: Vec<VideoCodec>,
    /// Subsets of `decode` / `encode` that run in hardware.
    pub hw_decode: Vec<VideoCodec>,
    pub hw_encode: Vec<VideoCodec>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoTuning {
    /// Empty or "auto", "av1", or "vp8". Applies to our sending direction only.
    pub send_codec: String,
    /// Zero = normal adaptation; otherwise a ceiling (not a promised wire rate).
    pub max_bps: u32,
    /// Zero = adaptive; otherwise 360, 540 or 720. Peer/budget limits still win.
    pub fixed_height: u32,
    /// Empty or "auto", "hw" or "sw": which decoder receives the peer's video, where this device
    /// has both kinds for the codec that arrives (VP8 on a phone with a hardware decoder). Local
    /// only: the offer's `hw_decode` (and so the peer's codec choice) is unchanged.
    pub recv_decoder: String,
    /// Empty, "auto" or "probe": Cut+probe (`SenderConfig::smooth_cut` and `burst_room`: probing,
    /// where the peer's offer says `burst_probe`), the default since 2026-10-20 (it fixed the
    /// moto's stall on a bumpy upload). For A/B calls (`docs/design/video-sim-tuning.md`): "plain"
    /// (the controller before, neither), "cut" (`smooth_cut` only) or "trend" (Cut+probe and the
    /// delay-trend detector).
    pub rate_control: String,
}

impl VideoTuning {
    pub fn codec(&self) -> Option<VideoCodec> {
        match self.send_codec.as_str() {
            "av1" => Some(VideoCodec::Av1),
            "vp8" => Some(VideoCodec::Vp8),
            _ => None,
        }
    }

    pub fn validate(&self, c: &LocalCodecs) -> Result<(), Error> {
        if !matches!(self.send_codec.as_str(), "" | "auto" | "av1" | "vp8") {
            return Err(Error::Invalid("unknown send codec".into()));
        }
        if !matches!(self.recv_decoder.as_str(), "" | "auto" | "hw" | "sw") {
            return Err(Error::Invalid("unknown decoder".into()));
        }
        if !matches!(self.rate_control.as_str(), "" | "auto" | "plain" | "cut" | "probe" | "trend") {
            return Err(Error::Invalid("unknown rate control".into()));
        }
        if self.codec().is_some_and(|codec| !c.encode.contains(&codec)) {
            return Err(Error::Invalid("this device cannot encode the requested codec".into()));
        }
        if !matches!(
            self.max_bps,
            0 | 500_000 | 1_000_000 | 2_000_000 | 3_000_000
        ) || !matches!(self.fixed_height, 0 | 360 | 540 | 720)
        {
            return Err(Error::Invalid("unsupported video tuning rate or resolution".into()));
        }
        Ok(())
    }

    pub fn filter(&self, c: &mut LocalCodecs) {
        if let Some(codec) = self.codec() {
            c.encode.retain(|&v| v == codec);
            c.hw_encode.retain(|&v| v == codec);
        }
    }

    pub fn configure(&self, cfg: &mut SenderConfig) {
        if self.max_bps != 0 {
            cfg.max_bps = self.max_bps;
            cfg.start_bps = cfg.start_bps.min(cfg.max_bps);
            for rung in &mut cfg.ladder.rungs {
                rung.max_bps = self.max_bps;
            }
        }
        cfg.ladder.fixed_rung = match self.fixed_height {
            360 => Some(1),
            540 => Some(2),
            720 => Some(3),
            _ => None,
        };
    }

    /// `rate_control` into the sender's config; probing only to a peer whose offer says
    /// `burst_probe` (an older receiver would decode the padding).
    pub fn configure_rate(&self, cfg: &mut SenderConfig, peer_probes: bool) {
        let rc = self.rate_control.as_str();
        cfg.smooth_cut = rc != "plain";
        cfg.burst_room = peer_probes && !matches!(rc, "plain" | "cut");
        if rc == "trend" {
            cfg.detector = video::detect::Detector::Trend;
        }
    }
}

/// Shared developer readout (Settings > Diagnostics while a call runs): the in-call sheet's rows
/// (`video_stat_rows`) without the connection, one "Label value" per line. Payload rate is
/// measured, not the encoder target or QUIC wire rate.
pub fn tuning_stats_label(s: VideoStats) -> String {
    if !s.tuning_problem.is_empty() {
        return s.tuning_problem;
    }
    stat_rows(s, 0, 0.0)
        .into_iter()
        .filter(|r| r.section != "link")
        .map(|r| {
            let who = match (r.section.as_str(), r.label.as_str()) {
                (_, l @ ("Your video" | "Their video")) => l.to_string(),
                ("send", l) => format!("Your {}", l.to_lowercase()),
                (_, l) => format!("Their {}", l.to_lowercase()),
            };
            format!("{who}: {}", r.value)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
