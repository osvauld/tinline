//! Camera capture: a thread that hands upright I420 frames to a callback until stopped.
//! Linux reads V4L2 directly (YUYV, NV12 or MJPEG, optional 720p with SD fallback);
//! elsewhere only the synthetic pattern exists for now. See docs/video.md, Desktop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use video::I420;

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;
pub const FPS: u32 = 30;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    #[default]
    Auto,
    PreferHd,
}

impl Quality {
    fn sizes(self) -> &'static [(u32, u32)] {
        match self {
            Self::Auto => &[(WIDTH, HEIGHT)],
            Self::PreferHd => &[(1280, 720), (WIDTH, HEIGHT)],
        }
    }
}

pub fn sending_label(w: u32, h: u32) -> String {
    let hd = w.max(h) >= 1280 && w.min(h) >= 720;
    format!("Sending {}{w}×{h}", if hd { "HD · " } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_preferences_keep_auto_and_bound_hd_fallback() {
        assert_eq!(Quality::default(), Quality::Auto);
        assert_eq!(Quality::Auto.sizes(), &[(640, 480)]);
        assert_eq!(Quality::PreferHd.sizes(), &[(1280, 720), (640, 480)]);
    }

    #[test]
    fn hd_label_uses_actual_dimensions_not_capture_preference() {
        assert_eq!(sending_label(1280, 720), "Sending HD · 1280×720");
        assert_eq!(sending_label(720, 1280), "Sending HD · 720×1280");
        assert_eq!(sending_label(640, 480), "Sending 640×480");
        assert_eq!(sending_label(960, 540), "Sending 960×540");
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A V4L2 capture node.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Device(std::path::PathBuf),
    /// `video::Pattern` at the requested capture size, 30 fps (tests only).
    Pattern,
}

/// The camera to use: the fake pattern if `fake` says so, else the first capture device with a
/// format we read. Opens each device briefly (no streaming, so no camera light).
pub fn find(fake: Option<&str>) -> Option<Source> {
    if fake == Some("pattern") {
        return Some(Source::Pattern);
    }
    #[cfg(target_os = "linux")]
    {
        v4l::find().map(Source::Device)
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// Enumerates capture nodes without streaming; called only by the explicit camera picker.
pub fn devices() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        v4l::nodes()
            .into_iter()
            .map(|p| p.display().to_string())
            .collect()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// An explicit selection never silently falls back to a different camera.
pub fn find_selected(fake: Option<&str>, selected: Option<&str>) -> Option<Source> {
    if fake == Some("pattern") || selected.is_none() {
        return find(fake);
    }
    devices()
        .into_iter()
        .find(|p| Some(p.as_str()) == selected)
        .map(|p| Source::Device(p.into()))
}

/// A running capture; dropping it stops the thread after its current frame (and closes the
/// device). Never joined, so a camera that stops delivering can't block the UI.
pub struct Camera {
    stop: Arc<AtomicBool>,
}

/// What the capture thread reports once it is streaming (logged by test-hooks builds).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Opened {
    pub format: String,
    pub width: u32,
    pub height: u32,
}

impl Camera {
    /// Starts capturing from `src`. `opened` runs once on the capture thread with the
    /// negotiated format (or the error that stopped it); `frame` runs per frame.
    pub fn start(
        src: Source,
        opened: impl FnOnce(Result<Opened, String>) + Send + 'static,
        frame: impl FnMut(I420) + Send + 'static,
    ) -> Camera {
        Self::start_with_quality(src, Quality::Auto, opened, frame)
    }

    pub fn start_with_quality(
        src: Source,
        quality: Quality,
        opened: impl FnOnce(Result<Opened, String>) + Send + 'static,
        frame: impl FnMut(I420) + Send + 'static,
    ) -> Camera {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let _ = std::thread::Builder::new()
            .name("tl-camera".into())
            .spawn(move || match src {
                Source::Pattern => pattern(&s, quality, opened, frame),
                #[cfg(target_os = "linux")]
                Source::Device(path) => v4l::capture(&path, &s, quality, opened, frame),
                #[cfg(not(target_os = "linux"))]
                Source::Device(_) => opened(Err("no camera support on this system yet".into())),
            });
        Camera { stop }
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn pattern(
    stop: &AtomicBool,
    quality: Quality,
    opened: impl FnOnce(Result<Opened, String>),
    mut frame: impl FnMut(I420),
) {
    let (w, h) = quality.sizes()[0];
    let mut p = video::Pattern::new(w as usize, h as usize);
    opened(Ok(Opened {
        format: "pattern".into(),
        width: w,
        height: h,
    }));
    let period = Duration::from_secs(1) / FPS;
    let mut next = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        frame(p.next_frame());
        next += period;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
}

#[cfg(target_os = "linux")]
mod v4l {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};

    use linuxvideo::format::{PixFormat, PixelFormat};
    use linuxvideo::{BufType, CapabilityFlags, Device, Fract};
    use video::convert::Range;
    use video::I420;

    use super::{Opened, Quality, FPS, WIDTH};
    use crate::frames::{self, Pix};

    const NV12: PixelFormat = PixelFormat::from_fourcc(*b"NV12");
    const MJPEG: PixelFormat = PixelFormat::from_fourcc(*b"MJPG");

    fn pix(f: PixelFormat) -> Option<Pix> {
        match f {
            PixelFormat::YUYV => Some(Pix::Yuyv),
            NV12 => Some(Pix::Nv12),
            MJPEG => Some(Pix::Mjpeg),
            _ => None,
        }
    }

    /// The formats of `dev` we read, best first.
    fn formats(dev: &Device) -> Vec<PixelFormat> {
        let mut v: Vec<PixelFormat> = dev
            .formats(BufType::VIDEO_CAPTURE)
            .filter_map(|f| f.ok())
            .map(|f| f.pixel_format())
            .filter(|f| pix(*f).is_some())
            .collect();
        v.sort_by_key(|f| match *f {
            PixelFormat::YUYV => 0,
            NV12 => 1,
            _ => 2,
        });
        v
    }

    fn capture_node(path: &Path) -> Option<Device> {
        let dev = Device::open(path).ok()?;
        let caps = dev.capabilities().ok()?.device_capabilities();
        (caps.contains(CapabilityFlags::VIDEO_CAPTURE)
            && caps.contains(CapabilityFlags::STREAMING)
            && !formats(&dev).is_empty())
        .then_some(dev)
    }

    pub fn nodes() -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir("/dev") else {
            return Vec::new();
        };
        let mut nodes: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("video"))
            .map(|e| e.path())
            .collect();
        nodes.sort_by_key(|p| {
            p.to_string_lossy()
                .trim_start_matches("/dev/video")
                .parse::<u32>()
                .unwrap_or(u32::MAX)
        });
        nodes
            .into_iter()
            .filter(|p| capture_node(p).is_some())
            .collect()
    }

    pub fn find() -> Option<PathBuf> {
        nodes().into_iter().next()
    }

    pub fn capture(
        path: &Path,
        stop: &AtomicBool,
        quality: Quality,
        opened: impl FnOnce(Result<Opened, String>),
        mut frame: impl FnMut(I420),
    ) {
        // A bounded set of mode attempts. Reopen between attempts: video_capture consumes Device.
        // Prefer MJPEG for HD: many USB webcams provide 720p30 only in compressed mode.
        let mut chosen = None;
        let mut last_error = "no supported camera mode".to_string();
        'sizes: for &(rw, rh) in quality.sizes() {
            let Some(dev) = capture_node(path) else { break };
            let mut modes = formats(&dev);
            drop(dev);
            if rw > WIDTH {
                modes.sort_by_key(|f| *f != MJPEG);
            }
            for mode in modes {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                let attempt = (|| -> Result<_, String> {
                    let dev = Device::open(path).map_err(|e| e.to_string())?;
                    let cap = dev
                        .video_capture(PixFormat::new(rw, rh, mode))
                        .map_err(|e| e.to_string())?;
                    let f = cap.format();
                    let kind =
                        pix(f.pixel_format()).ok_or("unsupported negotiated camera format")?;
                    let (w, h, stride) = (f.width(), f.height(), f.bytes_per_line());
                    if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 || w > 1280 || h > 720 {
                        return Err("camera mode exceeds capture bounds".into());
                    }
                    if rw > WIDTH && (w != rw || h != rh) {
                        return Err("HD mode unavailable".into());
                    }
                    let interval = cap.set_frame_interval(Fract::new(1, FPS)).ok();
                    // Avoid choosing 720p raw USB modes that only run at 5-10fps.
                    if rw > WIDTH
                        && interval.is_some_and(|i| {
                            i.numerator() == 0
                                || (i.denominator() as u64) < 24 * i.numerator() as u64
                        })
                    {
                        return Err("HD mode below 24fps; trying another format".into());
                    }
                    let stream = cap.into_stream().map_err(|e| e.to_string())?;
                    Ok((stream, kind, w as usize, h as usize, stride as usize))
                })();
                match attempt {
                    Ok(mode) => {
                        chosen = Some(mode);
                        break 'sizes;
                    }
                    Err(e) => last_error = e,
                }
            }
        }
        let Some((mut stream, kind, w, h, stride)) = chosen else {
            return opened(Err(format!("{}: {last_error}", path.display())));
        };
        opened(Ok(Opened {
            format: format!("{kind:?}"),
            width: w as u32,
            height: h as u32,
        }));
        // UVC YUYV is nominally limited range (BT.601); see docs/video.md for this camera.
        while !stop.load(Ordering::SeqCst) {
            let r = stream.dequeue(|buf| {
                if !buf.is_error() {
                    match frames::to_i420(kind, &buf, stride, w, h, Range::Limited) {
                        Ok(p) => frame(p),
                        Err(e) => eprintln!("camera: {e}"),
                    }
                }
                Ok(())
            });
            if let Err(e) = r {
                eprintln!("camera: {e}");
                break;
            }
        }
    }
}
