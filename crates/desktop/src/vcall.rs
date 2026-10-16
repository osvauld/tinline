//! Video in the call screen: camera on/off, the capture thread, and the pictures shown.
//! Remote frames are pulled on a ~30 Hz tick while video runs (see `App::subscription`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use iced::widget::image::Handle;
use iced::Task;
use p2pcore::{CallState, VideoInfo};

use super::{blocking, App, Msg};
use crate::camera::{self, Camera, Source};
use crate::frames::{self, Rgba};
use crate::video_surface::Picture;
use crate::{Ev, INIT};

#[derive(Debug, Clone)]
pub(super) enum Vm {
    Tick,
    ToggleCamera,
    Found(Option<Source>),
    /// The "video in calls" setting.
    Enabled(bool),
    PreferHd(bool),
    /// The in-call HD chip: this call only, not saved.
    ToggleHd,
    ScanCameras,
    Cameras(Vec<camera::Info>),
    SelectCamera(String),
}

#[derive(Default)]
pub(super) struct VideoUi {
    pub info: Option<VideoInfo>,
    /// The camera to use: None until looked up for this call, then Some(None) = no camera.
    pub source: Option<Option<Source>>,
    camera: Option<Camera>,
    /// Newest self-view picture, written by the capture thread.
    slot: Arc<Mutex<Option<Rgba>>>,
    pub local: Option<Handle>,
    pub remote: Option<Arc<Picture>>,
    /// Remote frames shown and the UI-thread cost per frame (pull), smoothed.
    shown: u64,
    cost_ms: f32,
    pub send_size: Option<(u32, u32)>,
    pub devices: Vec<camera::Info>,
    initial_sent: u64,
    /// The HD chip's choice for this call (None = follow Settings).
    pub hd_call: Option<bool>,
    /// The chip's short line and when it was said (shown 2.6 s in the picture label).
    pub hd_note: Option<(&'static str, Instant)>,
    was_paused: bool,
    stats_tick: u32,
}

impl VideoUi {
    /// Video runs in this call.
    pub fn negotiated(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.negotiated)
    }

    /// Either camera is on, so the call screen shows the video stage.
    pub fn live(&self) -> bool {
        self.info
            .as_ref()
            .is_some_and(|i| i.negotiated && (i.local_camera || i.remote_camera))
    }

    pub fn local_on(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.local_camera)
    }

    pub fn remote_on(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.remote_camera)
    }

    /// Sharper video is on for this call (the chip's choice, else Settings).
    pub fn hd_on(&self, prefer_hd_setting: bool) -> bool {
        self.hd_call.unwrap_or(prefer_hd_setting)
    }

    /// The chip's line while it is fresh.
    pub fn hd_line(&self) -> Option<&'static str> {
        self.hd_note.filter(|(_, at)| at.elapsed() < HD_NOTE).map(|(l, _)| l)
    }

    pub fn has_camera(&self) -> bool {
        matches!(self.source, Some(Some(_)))
    }
}

const HD_NOTE: std::time::Duration = std::time::Duration::from_millis(2600);

/// Test hook P2P_AUTO_CALL: placed once per run.
static AUTO_CALLED: AtomicBool = AtomicBool::new(false);

impl App {
    pub(super) fn video_update(&mut self, m: Vm) -> Task<Msg> {
        match m {
            Vm::Tick => self.video_tick(),
            Vm::ToggleCamera => {
                if self.video.negotiated() && self.video.has_camera() {
                    let on = !self.video.local_on();
                    if let Err(e) = self.node.set_camera(on) {
                        self.notice = Some(format!("Camera: {e}"));
                    }
                }
            }
            Vm::Found(src) => {
                // A chosen camera that is gone (unplugged, renumbered) is not swapped for another
                // one (`camera::find_selected`), so say why the camera stays off.
                if src.is_none() && self.settings.camera_device.is_some() {
                    self.notice = Some("Your chosen camera isn't connected. Pick one in Settings \u{2192} Video.".into());
                }
                self.video.source = Some(src);
                let intent = std::mem::take(&mut self.video_intent);
                if self.video.has_camera()
                    && !self.video.local_on()
                    && (intent || crate::test_env("P2P_AUTO_CAMERA").is_some())
                {
                    return self.video_update(Vm::ToggleCamera);
                }
                self.sync_camera();
            }
            Vm::Enabled(on) => {
                self.settings.no_video = !on;
                self.node.set_video_enabled(on);
                self.save_settings();
            }
            Vm::ScanCameras => return blocking(camera::infos, |d| Msg::Video(Vm::Cameras(d))),
            Vm::Cameras(devices) => self.video.devices = devices,
            Vm::SelectCamera(device) => {
                self.settings.camera_device = (device != super::DEFAULT_LABEL).then_some(device);
                self.save_settings();
                // Camera selection applies next call; don't interrupt an active capture.
            }
            Vm::ToggleHd => {
                let on = !self.video.hd_on(self.settings.prefer_hd);
                self.video.hd_call = Some(on);
                let paused = camera::hd_paused(on, self.video.send_size);
                self.video.was_paused = paused;
                self.video.hd_note = Some((camera::hd_line(on, paused), Instant::now()));
                if self.video.local_on() {
                    // Applies at once: stop the capture and start it again at the new size. The last self-view
                    // picture stays until the new camera's first frame; the label reads "Starting video…".
                    self.video.camera = None;
                    self.video.send_size = None;
                    self.sync_camera();
                }
            }
            Vm::PreferHd(on) => {
                self.settings.prefer_hd = on;
                self.save_settings();
            }
        }
        Task::none()
    }

    /// The core's video state for the current call changed.
    pub(super) fn video_state(&mut self, info: VideoInfo) -> Task<Msg> {
        if self
            .call
            .as_ref()
            .is_none_or(|c| c.info.call_id != info.call_id)
        {
            return Task::none();
        }
        let first = self.video.info.is_none();
        self.video.info = Some(info);
        if !self.video.remote_on() {
            self.video.remote = None;
        }
        self.sync_camera();
        if first && self.video.negotiated() && self.video.source.is_none() {
            let fake = crate::test_env("TINLINE_FAKE_CAMERA");
            let selected = self.settings.camera_device.clone();
            return blocking(
                move || camera::find_selected(fake.as_deref(), selected.as_deref()),
                |s| Msg::Video(Vm::Found(s)),
            );
        }
        Task::none()
    }

    /// Starts or stops the capture thread to match our camera state.
    fn sync_camera(&mut self) {
        let want = self.video.local_on();
        if !want {
            self.video.camera = None;
            self.video.local = None;
            self.video.send_size = None;
            self.video.slot.lock().unwrap().take();
            return;
        }
        let Some(Some(src)) = self.video.source.clone() else {
            return;
        };
        if self.video.camera.is_some() {
            return;
        }
        let (node, slot) = (self.node.clone(), self.video.slot.clone());
        let (node2, tx) = (self.node.clone(), INIT.get().unwrap().tx.clone());
        self.video.initial_sent = self.node.video_stats().map_or(0, |s| s.frames_sent);
        self.video.stats_tick = 0;
        self.video.camera = Some(Camera::start_with_quality(
            src,
            camera::quality_for(self.settings.prefer_hd, self.video.hd_call),
            move |r| match r {
                Ok(_o) => {
                    crate::tlog!("CAMERA {} {}x{}", _o.format, _o.width, _o.height);
                }
                Err(e) => {
                    let _ = node2.set_camera(false);
                    let _ = tx.send(Ev::AudioNotice(format!("Camera: {e}")));
                }
            },
            move |pic| {
                *slot.lock().unwrap() = Some(frames::self_view(&pic));
                frames::push(&node, pic);
            },
        ));
    }

    fn video_tick(&mut self) {
        if self.video.local_on() && !self.demo {
            if self.video.stats_tick == 0 {
                self.video.send_size = self.node.video_stats().and_then(|s| {
                    (s.frames_sent > self.video.initial_sent
                        && s.send_width > 0
                        && s.send_height > 0)
                        .then_some((s.send_width, s.send_height))
                });
            }
            self.video.stats_tick = (self.video.stats_tick + 1) % 30;
            // The ladder stepping below HD while HD is on: say so once.
            let paused = camera::hd_paused(self.video.hd_on(self.settings.prefer_hd), self.video.send_size);
            if paused && !self.video.was_paused {
                self.video.hd_note = Some((camera::hd_line(true, true), Instant::now()));
            }
            self.video.was_paused = paused;
        }
        let t = Instant::now();
        if let Some(r) = frames::pull(&self.node) {
            let r = Arc::new(r);
            self.video.remote = Some(r.clone());
            let ms = t.elapsed().as_secs_f32() * 1e3;
            let v = &mut self.video;
            v.cost_ms = if v.shown == 0 {
                ms
            } else {
                0.9 * v.cost_ms + 0.1 * ms
            };
            v.shown += 1;
            if v.shown.is_multiple_of(150) {
                crate::tlog!(
                    "VRENDER shown={} ms={:.2} {}x{}",
                    v.shown,
                    v.cost_ms,
                    r.width,
                    r.height
                );
            }
        }
        if let Some(l) = self.video.slot.lock().unwrap().take() {
            self.video.local = Some(Handle::from_rgba(l.width, l.height, l.data));
        }
    }

    /// The call ended: camera off, pictures gone.
    pub(super) fn video_reset(&mut self) {
        self.video = VideoUi::default();
    }

    /// Test hook P2P_AUTO_CALL=<contact name>: call them once online.
    pub(super) fn auto_call(&mut self) -> Task<Msg> {
        let Some(name) = crate::test_env("P2P_AUTO_CALL") else {
            return Task::none();
        };
        if !self.status.online || self.call.is_some() || AUTO_CALLED.load(Ordering::Relaxed) {
            return Task::none();
        }
        let Some(did) = self
            .contacts
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.did.clone())
        else {
            return Task::none();
        };
        AUTO_CALLED.store(true, Ordering::Relaxed);
        self.update(Msg::CallPressed(did))
    }

    /// Pull remote frames while video runs.
    pub(super) fn video_ticking(&self) -> bool {
        self.call
            .as_ref()
            .is_some_and(|c| c.state == CallState::Active)
            && self.video.live()
    }
}
