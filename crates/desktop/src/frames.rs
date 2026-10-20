//! Video frames in and out of the core: camera buffers -> I420 for `push_video_frame`, decoded
//! I420 for the video surface, and the mirrored self-view. Everything per-frame lives here, so
//! switching to the core's RGBA/YUYV entry points is one module.

use crate::video_surface::Picture;
use p2pcore::Node;
use video::convert::{self, Range, Yuv420};
use video::I420;

/// Pixel layouts the capture side hands over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pix {
    /// Packed 4:2:2, `Y0 U Y1 V` (YUY2), the format every UVC webcam offers.
    Yuyv,
    /// Y plane, then interleaved UV at half height.
    Nv12,
    /// JPEG-compressed frames from UVC cameras (often required for 720p30 over USB).
    Mjpeg,
}

/// An RGBA picture for `image::Handle::from_rgba`.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Width of the self-view picture (the PiP is never drawn larger).
const SELF_VIEW_WIDTH: usize = 320;

/// A camera buffer -> tightly packed I420. `stride` = bytes per line of the first plane (0 =
/// tight). Full-range input is squeezed to the limited range the encoder expects.
pub fn to_i420(
    pix: Pix,
    buf: &[u8],
    stride: usize,
    w: usize,
    h: usize,
    range: Range,
) -> Result<I420, String> {
    let mut out = match pix {
        Pix::Yuyv => convert::yuyv_to_i420(buf, if stride == 0 { 2 * w } else { stride }, w, h),
        Pix::Nv12 => {
            let stride = if stride == 0 { w } else { stride };
            let split = (stride * h).min(buf.len());
            let (y, uv) = buf.split_at(split);
            convert::yuv420_to_i420(&Yuv420::nv12(y, stride, uv, stride, w, h, false), None)
        }
        Pix::Mjpeg => return mjpeg_to_i420(buf, w, h),
    }
    .map_err(|e| e.to_string())?;
    if range == Range::Full {
        convert::full_to_limited(&mut out);
    }
    Ok(out)
}

/// Bounded JPEG decoding on the camera thread, never on the UI or codec thread.
fn mjpeg_to_i420(buf: &[u8], w: usize, h: usize) -> Result<I420, String> {
    if buf.len() > 8 * 1024 * 1024
        || w == 0
        || h == 0
        || w > 1920
        || h > 1080
        || w % 2 != 0
        || h % 2 != 0
    {
        return Err("unsupported MJPEG frame size".into());
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(buf), image::ImageFormat::Jpeg);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(w as u32);
    limits.max_image_height = Some(h as u32);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let rgb = reader.decode().map_err(|e| e.to_string())?.into_rgb8();
    if rgb.dimensions() != (w as u32, h as u32) {
        return Err("MJPEG dimensions differ from negotiated camera mode".into());
    }
    // RGB conversion already produces limited-range BT.601; don't squeeze it twice.
    convert::rgb_to_i420(rgb.as_raw(), w * 3, w, h, convert::RgbLayout::Rgb)
        .map_err(|e| e.to_string())
}

/// Mirrored, downscaled RGBA of our own camera frame for the picture-in-picture.
pub fn self_view(pic: &I420) -> Rgba {
    let small = if pic.width > SELF_VIEW_WIDTH {
        let h = (pic.height * SELF_VIEW_WIDTH / pic.width) & !1;
        convert::scale(pic, SELF_VIEW_WIDTH, h.max(2))
    } else {
        pic.clone()
    };
    rgba(&convert::transform(&small, convert::Rotation::R0, true))
}

pub fn rgba(pic: &I420) -> Rgba {
    Rgba {
        width: pic.width as u32,
        height: pic.height as u32,
        data: convert::to_rgba(pic),
    }
}

/// One upright camera frame to the call; false = not sent (see `Node::push_video_frame`).
pub fn push(node: &Node, pic: I420) -> bool {
    node.push_video_frame(pic.width as u32, pic.height as u32, 0, pic.data)
}

/// The peer's newest decoded frame, if there is a new one. Left as I420: the GPU converts it (`video_surface`).
pub fn pull(node: &Node) -> Option<Picture> {
    let f = node.pull_video_frame()?;
    Picture::new(f.width, f.height, f.data.into_inner())
}

#[cfg(test)]
#[path = "frames_tests.rs"]
mod tests;
