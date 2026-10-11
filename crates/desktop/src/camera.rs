//! Camera capture: a thread that hands upright I420 frames to a callback until stopped.
//! Linux reads V4L2 directly (YUYV or NV12, 640x480 at 30 fps or the driver's nearest);
//! elsewhere only the synthetic pattern exists for now. See docs/video.md, Desktop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use video::I420;

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;
pub const FPS: u32 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A V4L2 capture node.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Device(std::path::PathBuf),
    /// `video::Pattern` at 640x480, 30 fps (tests, machines without a camera).
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
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let _ = std::thread::Builder::new().name("tl-camera".into()).spawn(move || match src {
            Source::Pattern => pattern(&s, opened, frame),
            #[cfg(target_os = "linux")]
            Source::Device(path) => v4l::capture(&path, &s, opened, frame),
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

fn pattern(stop: &AtomicBool, opened: impl FnOnce(Result<Opened, String>), mut frame: impl FnMut(I420)) {
    let mut p = video::Pattern::new(WIDTH as usize, HEIGHT as usize);
    opened(Ok(Opened { format: "pattern".into(), width: WIDTH, height: HEIGHT }));
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

    use super::{Opened, FPS, HEIGHT, WIDTH};
    use crate::frames::{self, Pix};

    const NV12: PixelFormat = PixelFormat::from_fourcc(*b"NV12");

    fn pix(f: PixelFormat) -> Option<Pix> {
        match f {
            PixelFormat::YUYV => Some(Pix::Yuyv),
            NV12 => Some(Pix::Nv12),
            _ => None,
        }
    }

    /// The formats of `dev` we read, best first.
    fn formats(dev: &Device) -> Vec<PixelFormat> {
        let mut v: Vec<PixelFormat> =
            dev.formats(BufType::VIDEO_CAPTURE).filter_map(|f| f.ok()).map(|f| f.pixel_format()).filter(|f| pix(*f).is_some()).collect();
        v.sort_by_key(|f| *f != PixelFormat::YUYV);
        v
    }

    fn capture_node(path: &Path) -> Option<Device> {
        let dev = Device::open(path).ok()?;
        let caps = dev.capabilities().ok()?.device_capabilities();
        (caps.contains(CapabilityFlags::VIDEO_CAPTURE) && caps.contains(CapabilityFlags::STREAMING) && !formats(&dev).is_empty())
            .then_some(dev)
    }

    pub fn find() -> Option<PathBuf> {
        let mut nodes: Vec<PathBuf> = std::fs::read_dir("/dev")
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("video"))
            .map(|e| e.path())
            .collect();
        nodes.sort_by_key(|p| p.to_string_lossy().trim_start_matches("/dev/video").parse::<u32>().unwrap_or(u32::MAX));
        nodes.into_iter().find(|p| capture_node(p).is_some())
    }

    pub fn capture(path: &Path, stop: &AtomicBool, opened: impl FnOnce(Result<Opened, String>), mut frame: impl FnMut(I420)) {
        let dev = match capture_node(path) {
            Some(d) => d,
            None => return opened(Err(format!("{}: not a camera we can read", path.display()))),
        };
        let want = formats(&dev)[0];
        let cap = match dev.video_capture(PixFormat::new(WIDTH, HEIGHT, want)) {
            Ok(c) => c,
            Err(e) => return opened(Err(format!("{}: {e}", path.display()))),
        };
        let f = cap.format();
        let Some(kind) = pix(f.pixel_format()) else {
            return opened(Err(format!("{}: camera picked {:?}", path.display(), f.pixel_format())));
        };
        let (w, h, stride) = (f.width() as usize, f.height() as usize, f.bytes_per_line() as usize);
        let _ = cap.set_frame_interval(Fract::new(1, FPS));
        let mut stream = match cap.into_stream() {
            Ok(s) => s,
            Err(e) => return opened(Err(format!("{}: {e}", path.display()))),
        };
        opened(Ok(Opened { format: format!("{kind:?}"), width: w as u32, height: h as u32 }));
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
