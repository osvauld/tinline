//! Image posts (`docs/design/public-group.md` §5), the part that needs no node: turning the
//! picture a user picked into what a post carries. The sender does it, so every app stays thin
//! and every device gets the same bounded picture.
//!
//! [`prepare`]: decode (PNG, JPEG or WebP; EXIF orientation applied), flatten transparency onto
//! white, scale down so the long side is at most [`TARGET_SIDE`], and encode as JPEG under
//! [`TARGET_BYTES`] (lower quality first, then smaller). The 4x3 colour preview is the one 1:1
//! chat photos carry (`chat_core::photo::grid`).
//!
//! The runtime (blob store, serving gate, fetching, host limits) is `engine::images`.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{
    DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage, imageops::FilterType,
    metadata::Orientation,
};

use crate::Error;

/// Longest side of a posted picture, pixels.
pub const TARGET_SIDE: u32 = 1080;
/// A posted picture stays under this many bytes.
pub const TARGET_BYTES: usize = 300 * 1024;
/// Largest picture file we try to read (a phone's full-size photo fits easily).
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Largest side and decode memory we accept from the picked file.
const MAX_INPUT_SIDE: u32 = 16_384;
const MAX_DECODE_ALLOC: u64 = 512 * 1024 * 1024;
const QUALITIES: [u8; 5] = [85, 75, 65, 55, 45];

/// What a post carries of a picture.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub jpeg: Vec<u8>,
    pub w: u32,
    pub h: u32,
    pub preview: Vec<u8>,
}

fn bad(e: impl std::fmt::Display) -> Error {
    Error::Protocol(format!("image: {e}"))
}

/// The picture in `bytes`, ready to post: JPEG, long side at most `TARGET_SIDE`, under
/// `TARGET_BYTES`. Anything that is not a PNG, JPEG or WebP we can read is refused.
pub fn prepare(bytes: &[u8]) -> Result<Prepared, Error> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        return Err(bad("the file is empty or too large"));
    }
    let mut r = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(bad)?;
    if !matches!(
        r.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return Err(bad("only PNG, JPEG and WebP pictures can be posted"));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_INPUT_SIDE);
    limits.max_image_height = Some(MAX_INPUT_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    r.limits(limits);
    let mut dec = r.into_decoder().map_err(bad)?;
    let orient = dec.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(bad)?;
    img.apply_orientation(orient);
    if img.width() == 0 || img.height() == 0 {
        return Err(bad("the picture has no pixels"));
    }
    let mut rgb = flatten(&img);
    if rgb.width().max(rgb.height()) > TARGET_SIDE {
        rgb = scale(&rgb, TARGET_SIDE);
    }
    loop {
        for q in QUALITIES {
            let jpeg = encode(&rgb, q)?;
            if jpeg.len() < TARGET_BYTES {
                let preview = crate::chat::photo::grid(&DynamicImage::ImageRgb8(rgb.clone()));
                return Ok(Prepared {
                    jpeg,
                    w: rgb.width(),
                    h: rgb.height(),
                    preview,
                });
            }
        }
        // Still too large at the lowest quality (a very noisy picture): smaller.
        let side = rgb.width().max(rgb.height()) * 3 / 4;
        if side < 64 {
            return Err(bad("the picture does not fit the size limit"));
        }
        rgb = scale(&rgb, side);
    }
}

/// RGB with any transparency composited onto white (JPEG has no alpha).
fn flatten(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    })
}

/// Scaled so the long side is `side` (aspect kept).
fn scale(rgb: &RgbImage, side: u32) -> RgbImage {
    let (w, h) = rgb.dimensions();
    let (nw, nh) = if w >= h {
        (side, (h as u64 * side as u64 / w as u64).max(1) as u32)
    } else {
        ((w as u64 * side as u64 / h as u64).max(1) as u32, side)
    };
    image::imageops::resize(rgb, nw, nh, FilterType::Triangle)
}

fn encode(rgb: &RgbImage, quality: u8) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(rgb)
        .map_err(bad)?;
    Ok(out)
}

#[cfg(test)]
mod tests;
