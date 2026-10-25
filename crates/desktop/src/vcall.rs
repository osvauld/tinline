//! Video in the call screen: camera on/off, the capture thread, and the pictures shown.
//! Remote frames are pulled on a ~30 Hz tick while video runs (see `App::subscription`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::widget::image::Handle;
use iced::Task;
use p2pcore::{CallInfo, CallState, VideoInfo};

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
    /// A video call placed (`App::dial_video`): the camera it looked up first, and the call.
    Placed(Option<Source>, Result<CallInfo, String>),
    /// The "video in calls" setting.
    Enabled(bool),
    PreferHd(bool),
    /// The in-call HD chip: this call only, not saved.
    ToggleHd,
    ScanCameras,
    Cameras(Vec<camera::Info>),
    SelectCamera(String),
    #[cfg(feature = "video-tuning")]
    Tuning(p2pcore::VideoTuning),
    /// About this call's live controls: this call only, never saved.
    #[cfg(feature = "video-tuning")]
    Live(p2pcore::VideoLive),
}

#[derive(Default)]
pub(super) struct VideoUi {
    pub info: Option<VideoInfo>,
    /// The camera to use: None until looked up for this call, then Some(None) = no camera.
    pub source: Option<Option<Source>>,
    /// The lookup is running.
    looking: bool,
    /// My camera goes on with this call (placed as a video call, or answered with "Video"): the capture
    /// starts before video is decided (the core's `VideoInfo` once the call is active), so the camera is
    /// streaming by then. Once video is decided the core's state rules.
    early_camera: bool,
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
    /// The short line, when it was said and how long it shows in the picture label (the chip's echo 2.6 s, a
    /// sharper-video suggestion 6 s).
    pub hd_note: Option<(String, Instant, Duration)>,
    /// The core wants the camera at HD (`VideoStats::hd_auto`), refreshed about once a second.
    pub hd_auto: bool,
    was_paused: bool,
    /// The picture label for what I send ("HD \u{b7} 24 fps"), from the core, refreshed about once a second.
    pub send_label: String,
    /// The label for what I receive ("Their video \u{b7} 540p \u{b7} 24 fps"), refreshed about once a second.
    pub recv_label: String,
    /// Codec and hardware of each direction ("VP8 \u{b7} hardware"), for About this call; refreshed with `recv_label`.
    pub send_codec: String,
    pub recv_codec: String,
    recv_tick: u32,
    /// The last (width, height, visible) given to the core.
    view_sent: Option<(u32, u32, bool)>,
    stats_tick: u32,
    /// Why the last live control didn't apply (About this call's reserved line).
    #[cfg(feature = "video-tuning")]
    pub live_note: Option<String>,
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

    /// Sharper video is on for this call (the chip's choice, else Settings, else the core's).
    pub fn hd_on(&self, prefer_hd_setting: bool) -> bool {
        self.hd_call.unwrap_or(prefer_hd_setting || self.hd_auto)
    }

    /// The short line while it is fresh.
    pub fn hd_line(&self) -> Option<&str> {
        self.hd_note.as_ref().filter(|(_, at, ms)| at.elapsed() < *ms).map(|(l, _, _)| l.as_str())
    }

    fn say(&mut self, line: impl Into<String>, ms: Duration) {
        self.hd_note = Some((line.into(), Instant::now(), ms));
    }

    pub fn has_camera(&self) -> bool {
        matches!(self.source, Some(Some(_)))
    }
}

/// The chip's echo of a tap.
const HD_NOTE: Duration = Duration::from_millis(2600);

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
                        self.notice = Some(crate::i18n::tr_args("video_camera", &[("error", &(e).to_string())]));
                    }
                }
            }
            Vm::Found(src) => {
                self.video.looking = false;
                self.found(src);
                // Test hook: the camera goes on with video (the lookup now also runs while it rings).
                if self.video.has_camera()
                    && self.video.negotiated()
                    && !self.video.local_on()
                    && crate::test_env("P2P_AUTO_CAMERA").is_some()
                {
                    return self.video_update(Vm::ToggleCamera);
                }
                self.sync_camera();
            }
            Vm::Placed(src, r) => {
                self.video.looking = false;
                let video = r.as_ref().is_ok_and(|i| i.video);
                let t = self.update(Msg::CallStarted(r));
                if self.call.is_some() {
                    self.video.early_camera = video;
                    self.found(src);
                    self.sync_camera();
                }
                return t;
            }
            Vm::Enabled(on) => {
                self.settings.no_video = !on;
                self.node.set_video_enabled(on);
                self.save_settings();
            }
            Vm::ScanCameras => return blocking(camera::infos, |d| Msg::Video(Vm::Cameras(d))),
            Vm::Cameras(devices) => self.video.devices = devices,
            Vm::SelectCamera(device) => {
                self.settings.camera_device = (device != super::default_label()).then_some(device);
                self.save_settings();
                // Camera selection applies next call; don't interrupt an active capture.
            }
            Vm::ToggleHd => {
                let on = !self.video.hd_on(self.settings.prefer_hd);
                self.video.hd_call = Some(on);
                crate::tlog!("VHD chip={on}");
                self.node.set_video_hd(on);
                let paused = camera::hd_paused(on, self.video.send_size);
                self.video.was_paused = paused;
                self.video.say(camera::hd_line(on, paused), HD_NOTE);
                self.restart_camera();
            }
            #[cfg(feature = "video-tuning")]
            Vm::Live(live) => {
                self.video.live_note = self.node.set_video_live(live).err().map(|e| e.to_string());
            }
            #[cfg(feature = "video-tuning")]
            Vm::Tuning(tuning) => {
                if let Err(e) = self.node.set_video_tuning(tuning) {
                    self.notice = Some(crate::i18n::tr_args("video_video_tuning", &[("error", &(e).to_string())]));
                }
            }
            Vm::PreferHd(on) => {
                self.settings.prefer_hd = on;
                self.save_settings();
            }
        }
        Task::none()
    }

    /// The camera lookup's answer for this call. A chosen camera that is gone (unplugged, renumbered) is not
    /// swapped for another one (`camera::find_selected`), so say why the camera stays off; with no camera at
    /// all, a camera that went on with the call (video call, "Video") goes off again.
    fn found(&mut self, src: Option<Source>) {
        if src.is_none() && self.settings.camera_device.is_some() {
            self.notice = Some(crate::i18n::tr_str("video_your_chosen_camera_isnt").into());
        }
        if src.is_none() && (self.video.early_camera || self.video.local_on()) {
            self.video.early_camera = false;
            let _ = self.node.set_camera(false);
        }
        self.video.source = Some(src);
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
        // A `VideoInfo` can come before the call is active (my camera on while it rings): video is decided
        // with the first negotiated one.
        let first = !self.video.negotiated();
        self.video.info = Some(info);
        if !self.video.remote_on() {
            self.video.remote = None;
        }
        self.sync_camera();
        if first && self.video.negotiated() {
            // Test hook: the camera found while it rang goes on with video.
            if self.video.has_camera() && !self.video.local_on() && crate::test_env("P2P_AUTO_CAMERA").is_some() {
                return self.video_update(Vm::ToggleCamera);
            }
            return self.find_camera();
        }
        Task::none()
    }

    /// Looks the camera up for this call (once; opens each capture node briefly without streaming, so no
    /// camera light), unless it is known, being looked up, or video is off.
    pub(super) fn find_camera(&mut self) -> Task<Msg> {
        if self.video.source.is_some() || self.video.looking || self.settings.no_video {
            return Task::none();
        }
        self.video.looking = true;
        let fake = crate::test_env("TINLINE_FAKE_CAMERA");
        let selected = self.settings.camera_device.clone();
        blocking(
            move || camera::find_selected(fake.as_deref(), selected.as_deref()),
            |s| Msg::Video(Vm::Found(s)),
        )
    }

    /// The video call button: looks the camera up, then places a video call (the core turns my camera on from
    /// the start, so the capture starts while it rings and is warm when video runs; docs/video.md "Starting
    /// the camera"). Without a camera it is still a video call, with mine off.
    pub(super) fn dial_video(&mut self, did: String) -> Task<Msg> {
        if !self.may_dial(&did) {
            return Task::none();
        }
        let node = self.node.clone();
        let fake = crate::test_env("TINLINE_FAKE_CAMERA");
        let selected = self.settings.camera_device.clone();
        self.video.looking = true;
        blocking(
            move || {
                let src = camera::find_selected(fake.as_deref(), selected.as_deref());
                (src, node.video_call(did).map_err(|e| e.to_string()))
            },
            |(src, r)| Msg::Video(Vm::Placed(src, r)),
        )
    }

    /// "Video" on a ringing video call: my camera on (the core takes it while ringing; the peer hears it once
    /// the call is active), the capture started, then answer. Plain Answer keeps it off.
    pub(super) fn answer_with_video(&mut self) -> Task<Msg> {
        if self.call.is_none() {
            return Task::none();
        }
        if !matches!(self.video.source, Some(None)) && self.node.set_camera(true).is_ok() {
            self.video.early_camera = true;
            self.sync_camera();
        }
        let find = self.find_camera();
        let answer = self.update(Msg::Answer);
        Task::batch([find, answer])
    }

    /// Starts or stops the capture thread to match our camera state.
    fn sync_camera(&mut self) {
        // Until video is decided, a camera that went on with the call (video call, "Video"); from then on the
        // core's state, so a call without video (the peer has none) never captures.
        let active = self.call.as_ref().is_some_and(|c| c.state == CallState::Active);
        let want = if self.video.negotiated() { self.video.local_on() } else { !active && self.video.early_camera };
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
            camera::quality_for(self.video.hd_on(self.settings.prefer_hd)),
            move |r| match r {
                Ok(_o) => {
                    crate::tlog!("CAMERA {} {}x{}", _o.format, _o.width, _o.height);
                }
                Err(e) => {
                    let _ = node2.set_camera(false);
                    let _ = tx.send(Ev::AudioNotice(crate::i18n::tr_args("video_camera", &[("error", &(e).to_string())])));
                }
            },
            move |pic| {
                *slot.lock().unwrap() = Some(frames::self_view(&pic));
                frames::push(&node, pic);
            },
        ));
    }

    fn video_tick(&mut self) {
        if self.video.recv_tick == 0 {
            let stats = self.node.video_stats();
            self.video.recv_label = stats.as_ref().map(p2pcore::video_recv_label).unwrap_or_default();
            let codec = |c: &str, hw| p2pcore::video_codec_label(c.into(), hw);
            (self.video.send_codec, self.video.recv_codec) = stats.as_ref().map_or_else(Default::default, |s| {
                (codec(&s.send_codec, s.send_hardware), codec(&s.recv_codec, s.recv_hardware))
            });
        }
        self.video.recv_tick = (self.video.recv_tick + 1) % 30;
        if self.video.local_on() {
            if self.video.stats_tick == 0 {
                let stats = self.node.video_stats();
                self.video.send_label = stats.as_ref().map(p2pcore::video_send_label).unwrap_or_default();
                self.hd_auto_tick(stats.as_ref());
                self.video.send_size = stats.and_then(|s| {
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
                self.video.say(camera::hd_line(true, true), HD_NOTE);
            }
            self.video.was_paused = paused;
        }
        // The remote picture's drawn size, on first layout and when it changes (the core rounds
        // it to a ladder rung and holds a shrink for a second).
        if let Some(d) = crate::video_surface::drawn()
            && self.video.view_sent != Some(d)
        {
            self.video.view_sent = Some(d);
            self.node.set_video_view(d.0, d.1, d.2);
        }
        let t = Instant::now();
        if let Some(r) = frames::pull(&self.node) {
            let r = Arc::new(r);
            self.video.remote = Some(r.clone());
            let ms = t.elapsed().as_secs_f32() * 1e3;
            self.node.report_video_render_ms(ms);
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

    /// Once a second while my camera sends: follow the core's HD choice (unless the chip or Settings decide) and
    /// keep the core told what the camera captures (docs/video.md "Sharper video").
    fn hd_auto_tick(&mut self, stats: Option<&p2pcore::VideoStats>) {
        let before = self.video.hd_on(self.settings.prefer_hd);
        self.video.hd_auto = stats.is_some_and(|s| s.hd_auto);
        let hd_on = self.video.hd_on(self.settings.prefer_hd);
        self.node.set_video_hd(hd_on);
        if hd_on != before {
            crate::tlog!("VHD auto={hd_on}");
            self.restart_camera();
        }
    }

    /// A new HD choice applies at once: stop the capture and start it again at the new size. The last self-view
    /// picture stays until the new camera's first frame; the label reads "Starting video…".
    fn restart_camera(&mut self) {
        if self.video.local_on() {
            self.video.camera = None;
            self.video.send_size = None;
            self.sync_camera();
        }
    }

    /// The call ended: camera off, pictures gone.
    pub(super) fn video_reset(&mut self) {
        self.video = VideoUi::default();
        crate::video_surface::forget_drawn();
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

#[cfg(test)]
#[path = "vcall_tests.rs"]
mod tests;
