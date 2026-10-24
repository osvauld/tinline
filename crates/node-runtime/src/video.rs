//! The 1:1 call's video, as the node drives it: thin glue over `call_video` and the call slot.

use super::*;
use call_video::Pulled;
use proto::VideoOffer;

impl NodeCtx {
    /// Video is enabled on this node (`Node::set_video_enabled`).
    pub fn video_enabled(&self) -> bool {
        self.video.enabled()
    }

    /// Our offer, when video is enabled on this node.
    pub fn video_offer(&self, call: &Call) -> Option<VideoOffer> {
        self.video.offer_for(&call.video)
    }

    pub fn emit_video(&self, call: &Call) {
        self.video.emit(&call.info.call_id, &call.video);
    }

    /// The call went active. With both offers present the video streams start on `conn`;
    /// either way the UI hears whether there is video.
    pub fn video_decided(&self, call: &Arc<Call>, conn: &Connection, ours: Option<VideoOffer>, theirs: Option<VideoOffer>) {
        call.video.decide(&self.handle, conn, ours, theirs, &self.video, &call.info.call_id);
    }

    /// The peer's `Msg::Video`.
    pub fn remote_camera(&self, call: &Call, on: bool) {
        if call.video.set_remote_camera(on) {
            self.emit_video(call);
        }
    }

    /// The peer's `Msg::VideoCodec`: its video is `codec` from its next keyframe.
    pub fn remote_codec(&self, call: &Call, codec: proto::VideoCodec) {
        call.video.remote_codec(codec);
    }

    pub fn current_video_call(&self) -> Option<Arc<Call>> {
        self.live.lock().call.clone()
    }

    /// `push_video_frame_yuv` from borrowed planes: each copied once, only the bytes the frame
    /// spans.
    #[allow(clippy::too_many_arguments)]
    pub fn push_yuv_planes(
        &self,
        width: u32,
        height: u32,
        rotation: u32,
        y: &[u8],
        y_stride: usize,
        u: &[u8],
        v: &[u8],
        uv_stride: usize,
        uv_pixel_stride: usize,
        full_range: bool,
    ) -> bool {
        let Some(call) = self.current_video_call() else { return false };
        call.video.push_yuv_planes(width, height, rotation, y, y_stride, u, v, uv_stride, uv_pixel_stride, full_range)
    }

    pub fn pull_rgba_into(&self, dst: &mut [u8]) -> Pulled {
        let Some(call) = self.current_video_call() else { return Pulled::None };
        call.video.pull_rgba_into(dst)
    }
}
