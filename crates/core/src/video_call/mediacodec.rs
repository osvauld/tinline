//! The debug self-test of the Android MediaCodec VP8 path. The codec itself lives in
//! `video::mediacodec` (feature `mediacodec`); this keeps the uniffi export in p2pcore.

/// Debug self-test of the MediaCodec VP8 path (no camera, no call): synthetic 30 fps frames
/// through `MediaCodecVp8Encoder` for `seconds` with the bitrate schedule 2.5 Mbps, 1.0 Mbps at
/// 10 s, keyframe request at 15 s, 2.5 Mbps at 20 s; packets go to `MediaCodecVp8Decoder` on
/// another thread and decoded pictures are compared with the source. Returns one line per
/// second plus summary lines. `wait_ms` < 0 = the encoder's default wait for its output;
/// `software`: Google's software encoder instead of the default one; `fps` (1-30, 0 = 30): frames
/// at that tier with the call's scaled bitrate. Debug builds run it via DebugReceiver
/// `mediacodec_vp8` (`--ez sw true`, `--ei fps 15`).
#[cfg(feature = "dev-tools")]
#[uniffi::export]
pub fn mediacodec_vp8_selftest(
    seconds: u32,
    width: u32,
    height: u32,
    wait_ms: i32,
    software: bool,
    fps: u32,
) -> std::result::Result<String, crate::Error> {
    video::mediacodec::mediacodec_vp8_selftest(seconds, width, height, wait_ms, software, fps).map_err(crate::Error::Protocol)
}
