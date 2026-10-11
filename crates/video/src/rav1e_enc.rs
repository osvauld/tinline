//! rav1e encoder, kept only to compare against libaom: it holds ~4 frames and is far too slow
//! for calls (see docs/video.md).

use rav1e::prelude::*;

use crate::{EncoderConfig, Error, I420, Packet, Result, VideoEncoder};

pub struct Encoder {
    ctx: Context<u8>,
    width: usize,
    height: usize,
}

impl Encoder {
    pub fn new(cfg: &EncoderConfig) -> Result<Self> {
        let mut speed = SpeedSettings::from_preset(cfg.speed);
        // Realtime: every frame is encoded and emitted before the next arrives.
        speed.rdo_lookahead_frames = 1;
        speed.scene_detection_mode = SceneDetectionSpeed::None;
        let enc = rav1e::config::EncoderConfig {
            width: cfg.width,
            height: cfg.height,
            time_base: Rational::new(1, cfg.fps as u64),
            bit_depth: 8,
            chroma_sampling: ChromaSampling::Cs420,
            low_latency: true,
            error_resilient: true,
            min_key_frame_interval: 0,
            max_key_frame_interval: cfg.keyint,
            bitrate: cfg.bitrate_bps as i32,
            tiles: cfg.tiles,
            speed_settings: speed,
            ..Default::default()
        };
        let ctx = Config::new()
            .with_encoder_config(enc)
            .with_threads(cfg.threads)
            .new_context()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self { ctx, width: cfg.width, height: cfg.height })
    }

    pub fn encode(&mut self, pic: &I420, key: bool) -> Result<Vec<Packet>> {
        let want = I420::len(self.width, self.height);
        if pic.width != self.width || pic.height != self.height || pic.data.len() != want {
            return Err(Error::BadFrame(pic.data.len(), want));
        }
        let mut f = self.ctx.new_frame();
        let (y, u, v) = pic.planes();
        let (cw, _) = I420::chroma(self.width, self.height);
        f.planes[0].copy_from_raw_u8(y, self.width, 1);
        f.planes[1].copy_from_raw_u8(u, cw, 1);
        f.planes[2].copy_from_raw_u8(v, cw, 1);
        let params = FrameParameters {
            frame_type_override: if key { FrameTypeOverride::Key } else { FrameTypeOverride::No },
            ..Default::default()
        };
        self.ctx.send_frame((f, params)).map_err(|e| Error::Encode(format!("{e:?}")))?;
        self.drain()
    }

    /// Packets for frames still inside the encoder (none in realtime config, but cheap to check).
    pub fn flush(&mut self) -> Result<Vec<Packet>> {
        self.ctx.flush();
        self.drain()
    }

    fn drain(&mut self) -> Result<Vec<Packet>> {
        let mut out = Vec::new();
        loop {
            match self.ctx.receive_packet() {
                Ok(p) => out.push(Packet {
                    key: p.frame_type == FrameType::KEY,
                    frame: p.input_frameno,
                    data: p.data,
                }),
                Err(EncoderStatus::Encoded) => continue,
                Err(EncoderStatus::NeedMoreData | EncoderStatus::LimitReached) => break,
                Err(e) => return Err(Error::Encode(format!("{e:?}"))),
            }
        }
        Ok(out)
    }
}

impl VideoEncoder for Encoder {
    fn encode(&mut self, pic: &I420, key: bool) -> Result<Vec<Packet>> {
        Encoder::encode(self, pic, key)
    }

    fn set_bitrate(&mut self, _bps: u32) -> Result<()> {
        // rav1e fixes the bitrate at creation; the comparison encoder doesn't adapt.
        Ok(())
    }
}

