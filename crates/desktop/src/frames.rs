//! Video frames in and out of the core: camera buffers -> I420 for `push_video_frame`, decoded
//! I420 -> RGBA for iced images, and the mirrored self-view. Everything per-frame lives here, so
//! switching to the core's RGBA/YUYV entry points is one module.

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
pub fn to_i420(pix: Pix, buf: &[u8], stride: usize, w: usize, h: usize, range: Range) -> Result<I420, String> {
    let mut out = match pix {
        Pix::Yuyv => convert::yuyv_to_i420(buf, if stride == 0 { 2 * w } else { stride }, w, h),
        Pix::Nv12 => {
            let stride = if stride == 0 { w } else { stride };
            let split = (stride * h).min(buf.len());
            let (y, uv) = buf.split_at(split);
            convert::yuv420_to_i420(&Yuv420::nv12(y, stride, uv, stride, w, h, false), None)
        }
    }
    .map_err(|e| e.to_string())?;
    if range == Range::Full {
        convert::full_to_limited(&mut out);
    }
    Ok(out)
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
    Rgba { width: pic.width as u32, height: pic.height as u32, data: convert::to_rgba(pic) }
}

/// One upright camera frame to the call; false = not sent (see `Node::push_video_frame`).
pub fn push(node: &Node, pic: I420) -> bool {
    node.push_video_frame(pic.width as u32, pic.height as u32, 0, pic.data)
}

/// The peer's newest decoded frame as RGBA, if there is a new one.
pub fn pull(node: &Node) -> Option<Rgba> {
    let f = node.pull_video_frame()?;
    let pic = I420 { width: f.width as usize, height: f.height as usize, data: f.data.into_inner() };
    (pic.data.len() == I420::len(pic.width, pic.height)).then(|| rgba(&pic))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x2 YUYV picture: Y ramps, U/V differ per row so the row-pair average shows.
    fn yuyv(stride: usize) -> Vec<u8> {
        let mut b = vec![0xEE; stride * 2];
        for (y, row) in [[10, 100, 20, 200, 30, 102, 40, 202], [50, 110, 60, 210, 70, 112, 80, 212]].iter().enumerate() {
            b[y * stride..y * stride + 8].copy_from_slice(row);
        }
        b
    }

    #[test]
    fn yuyv_tight_and_padded() {
        for stride in [8, 16] {
            let p = to_i420(Pix::Yuyv, &yuyv(stride), if stride == 8 { 0 } else { stride }, 4, 2, Range::Limited).unwrap();
            assert_eq!((p.width, p.height), (4, 2));
            let (y, u, v) = p.planes();
            assert_eq!(y, [10, 20, 30, 40, 50, 60, 70, 80]);
            assert_eq!(u, [105, 107]);
            assert_eq!(v, [205, 207]);
        }
    }

    #[test]
    fn nv12_padded() {
        let (w, h, stride) = (4, 2, 6);
        let mut b = vec![0xEE; stride * h + stride];
        b[..4].copy_from_slice(&[1, 2, 3, 4]);
        b[stride..stride + 4].copy_from_slice(&[5, 6, 7, 8]);
        b[stride * h..stride * h + 4].copy_from_slice(&[100, 200, 101, 201]);
        let p = to_i420(Pix::Nv12, &b, stride, w, h, Range::Limited).unwrap();
        assert_eq!(p.planes(), (&[1, 2, 3, 4, 5, 6, 7, 8][..], &[100, 101][..], &[200, 201][..]));
    }

    #[test]
    fn full_range_is_squeezed() {
        let mut b = vec![0u8; 8];
        b.copy_from_slice(&[0, 128, 255, 128, 0, 128, 255, 128]);
        let p = to_i420(Pix::Yuyv, &b, 0, 4, 1, Range::Full).unwrap();
        assert_eq!(p.planes().0, [16, 235, 16, 235]);
    }

    #[test]
    fn short_buffers_are_errors() {
        assert!(to_i420(Pix::Yuyv, &[0; 15], 0, 4, 2, Range::Limited).is_err());
        assert!(to_i420(Pix::Nv12, &[0; 10], 0, 4, 2, Range::Limited).is_err());
    }

    #[test]
    fn self_view_is_small_and_mirrored() {
        let mut p = I420::new(640, 480);
        // Left half white, right half black.
        for row in p.data[..640 * 480].chunks_exact_mut(640) {
            row[..320].fill(235);
            row[320..].fill(16);
        }
        p.data[640 * 480..].fill(128);
        let v = self_view(&p);
        assert_eq!((v.width, v.height), (320, 240));
        assert_eq!(v.data.len(), 320 * 240 * 4);
        // Mirrored: black on the left now.
        assert!(v.data[0] < 10 && v.data[319 * 4] > 245);
    }
}
