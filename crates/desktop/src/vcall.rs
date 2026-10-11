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
use crate::{Ev, INIT};

#[derive(Debug, Clone)]
pub(super) enum Vm {
    Tick,
    ToggleCamera,
    Found(Option<Source>),
    /// The "video in calls" setting.
    Enabled(bool),
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
    pub remote: Option<Handle>,
    /// Remote frames shown and the UI-thread cost per frame (pull + RGBA + handle), smoothed.
    shown: u64,
    cost_ms: f32,
}

impl VideoUi {
    /// Video runs in this call.
    pub fn negotiated(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.negotiated)
    }

    /// Either camera is on, so the call screen shows the video stage.
    pub fn live(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.negotiated && (i.local_camera || i.remote_camera))
    }

    pub fn local_on(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.local_camera)
    }

    pub fn remote_on(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.remote_camera)
    }

    pub fn has_camera(&self) -> bool {
        matches!(self.source, Some(Some(_)))
    }
}

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
                self.video.source = Some(src);
                if self.video.has_camera() && !self.video.local_on() && crate::test_env("P2P_AUTO_CAMERA").is_some() {
                    return self.video_update(Vm::ToggleCamera);
                }
                self.sync_camera();
            }
            Vm::Enabled(on) => {
                self.settings.no_video = !on;
                self.node.set_video_enabled(on);
                self.save_settings();
            }
        }
        Task::none()
    }

    /// The core's video state for the current call changed.
    pub(super) fn video_state(&mut self, info: VideoInfo) -> Task<Msg> {
        if !self.call.as_ref().is_some_and(|c| c.info.call_id == info.call_id) {
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
            return blocking(move || camera::find(fake.as_deref()), |s| Msg::Video(Vm::Found(s)));
        }
        Task::none()
    }

    /// Starts or stops the capture thread to match our camera state.
    fn sync_camera(&mut self) {
        let want = self.video.local_on();
        if !want {
            self.video.camera = None;
            self.video.local = None;
            self.video.slot.lock().unwrap().take();
            return;
        }
        let Some(Some(src)) = self.video.source.clone() else { return };
        if self.video.camera.is_some() {
            return;
        }
        let (node, slot) = (self.node.clone(), self.video.slot.clone());
        let (node2, tx) = (self.node.clone(), INIT.get().unwrap().tx.clone());
        self.video.camera = Some(Camera::start(
            src,
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
        let t = Instant::now();
        if let Some(r) = frames::pull(&self.node) {
            self.video.remote = Some(Handle::from_rgba(r.width, r.height, r.data));
            let ms = t.elapsed().as_secs_f32() * 1e3;
            let v = &mut self.video;
            v.cost_ms = if v.shown == 0 { ms } else { 0.9 * v.cost_ms + 0.1 * ms };
            v.shown += 1;
            if v.shown % 150 == 0 {
                crate::tlog!("VRENDER shown={} ms={:.2} {}x{}", v.shown, v.cost_ms, r.width, r.height);
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
        let Some(name) = crate::test_env("P2P_AUTO_CALL") else { return Task::none() };
        if !self.status.online || self.call.is_some() || AUTO_CALLED.load(Ordering::Relaxed) {
            return Task::none();
        }
        let Some(did) = self.contacts.iter().find(|c| c.name == name).map(|c| c.did.clone()) else { return Task::none() };
        AUTO_CALLED.store(true, Ordering::Relaxed);
        self.update(Msg::CallPressed(did))
    }

    /// Pull remote frames while video runs.
    pub(super) fn video_ticking(&self) -> bool {
        self.call.as_ref().is_some_and(|c| c.state == CallState::Active) && self.video.live()
    }
}
