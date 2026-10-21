//! What a photo message says about its picture before anyone downloads it (`tinline/chat/5`): the size it is shown at
//! (EXIF orientation applied, so a phone's portrait shot is portrait) and a 4x3 grid of its colours (36 bytes, RGB),
//! which the apps scale up smoothly as a soft preview until the photo arrives. Chats can then draw the photo's box at
//! its real shape from the start, so nothing moves when it lands (decided 2026-10-14, playground `?lab=photos`).
//!
//! Best effort: a file that isn't a picture we can read gets neither (0x0, no preview) and is sent as before.
use image::{DynamicImage, ImageDecoder, ImageReader, metadata::Orientation};
use std::path::Path;

/// Preview grid: columns x rows, RGB (the day doc bounds a message's preview by `PREVIEW_LEN`).
pub const PREVIEW_W: u32 = 4;
pub const PREVIEW_H: u32 = 3;
pub const PREVIEW_LEN: usize = (PREVIEW_W * PREVIEW_H * 3) as usize;
/// Larger pictures still get their size; decoding them only for the preview isn't worth the memory.
const MAX_PREVIEW_PIXELS: u64 = 64_000_000;

/// The size a picture is shown at (orientation applied) and its 4x3 colour grid; `None` if `path` isn't a picture
/// we read (png, jpeg, webp). The preview is empty when the picture is too large to decode for it.
pub fn describe(path: &Path, mime: &str) -> Option<(u32, u32, Vec<u8>)> {
    if !mime.starts_with("image/") {
        return None;
    }
    let mut dec = ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let orient = dec.orientation().unwrap_or(Orientation::NoTransforms);
    let (w, h) = dec.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let turned = matches!(orient, Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH);
    let (sw, sh) = if turned { (h, w) } else { (w, h) };
    let preview = if (w as u64) * (h as u64) <= MAX_PREVIEW_PIXELS {
        DynamicImage::from_decoder(dec).ok().map(|mut img| {
            img.apply_orientation(orient);
            grid(&img)
        })
    } else {
        None
    };
    Some((sw, sh, preview.unwrap_or_default()))
}

/// The average colour of each cell of a 4x3 grid over the picture (RGB, row by row).
fn grid(img: &DynamicImage) -> Vec<u8> {
    let rgb = img.thumbnail(64, 64).to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut out = Vec::with_capacity(PREVIEW_LEN);
    for gy in 0..PREVIEW_H {
        for gx in 0..PREVIEW_W {
            let (x0, x1) = (gx * w / PREVIEW_W, ((gx + 1) * w / PREVIEW_W).max(gx * w / PREVIEW_W + 1));
            let (y0, y1) = (gy * h / PREVIEW_H, ((gy + 1) * h / PREVIEW_H).max(gy * h / PREVIEW_H + 1));
            let mut sum = [0u64; 3];
            let mut n = 0u64;
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let p = rgb.get_pixel(x, y).0;
                    for c in 0..3 {
                        sum[c] += p[c] as u64;
                    }
                    n += 1;
                }
            }
            for s in sum {
                out.push((s / n.max(1)) as u8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
