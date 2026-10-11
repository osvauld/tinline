//! Minimal Y4M (YUV4MPEG2, 8-bit 4:2:0) reader and a box-filter scaler, for benchmarking and
//! simulating with real camera footage. Test tooling only: input is local files.

use std::io::{BufRead, BufReader, Read};

use crate::I420;

pub struct Y4m<R> {
    r: BufReader<R>,
    pub width: usize,
    pub height: usize,
    /// Frames per second as (num, den).
    pub rate: (u32, u32),
}

impl<R: Read> Y4m<R> {
    pub fn new(r: R) -> std::io::Result<Self> {
        let mut r = BufReader::new(r);
        let mut line = String::new();
        r.read_line(&mut line)?;
        let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
        let mut it = line.split_whitespace();
        if it.next() != Some("YUV4MPEG2") {
            return Err(bad("not a y4m file"));
        }
        let (mut width, mut height, mut rate) = (0, 0, (30, 1));
        for tok in it {
            let (k, v) = tok.split_at(1);
            match k {
                "W" => width = v.parse().map_err(|_| bad("width"))?,
                "H" => height = v.parse().map_err(|_| bad("height"))?,
                "F" => {
                    let (n, d) = v.split_once(':').ok_or_else(|| bad("rate"))?;
                    rate = (n.parse().map_err(|_| bad("rate"))?, d.parse().map_err(|_| bad("rate"))?);
                }
                "C" if !v.starts_with("420") => return Err(bad("only 4:2:0 supported")),
                _ => {}
            }
        }
        if width == 0 || height == 0 {
            return Err(bad("missing size"));
        }
        Ok(Self { r, width, height, rate })
    }

    /// Next frame, or `None` at end of file (including a truncated last frame).
    pub fn next_frame(&mut self) -> Option<I420> {
        let mut line = String::new();
        if self.r.read_line(&mut line).ok()? == 0 || !line.starts_with("FRAME") {
            return None;
        }
        let mut img = I420::new(self.width, self.height);
        self.r.read_exact(&mut img.data).ok()?;
        Some(img)
    }
}

pub use crate::convert::scale;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frames_and_scales() {
        let mut f = b"YUV4MPEG2 W4 H2 F30:1 Ip C420jpeg\n".to_vec();
        for v in [10u8, 200] {
            f.extend_from_slice(b"FRAME\n");
            f.extend(std::iter::repeat_n(v, I420::len(4, 2)));
        }
        f.extend_from_slice(b"FRAME\n\x01"); // truncated
        let mut y = Y4m::new(&f[..]).unwrap();
        assert_eq!((y.width, y.height, y.rate), (4, 2, (30, 1)));
        assert_eq!(y.next_frame().unwrap().data[0], 10);
        let second = y.next_frame().unwrap();
        assert!(y.next_frame().is_none());
        let half = scale(&second, 2, 1);
        assert_eq!(half.data, vec![200; I420::len(2, 1)]);
    }
}
