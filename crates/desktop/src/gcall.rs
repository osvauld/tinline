//! Group calls on this computer (GroupCallScreen.svelte, gcall.svelte.ts): start or join a group's call, the roster,
//! mute, leave, minimise to a strip, and the calls running in my groups. The core does the calling and the audio
//! (`audio::Ctl::on_group_call` runs the mic and speaker); this is the presentation.
//!
//! Not shown yet because the core doesn't expose them (SCREENS-PORT-STATUS.md): who is speaking, other people's mute,
//! each person's line to the host (§ 6h), how many joined a call I'm not in, and what kind of device the host is.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::Task;
use p2pcore::{GroupCallEvent, GroupCallEventKind};

use super::{blocking, App, Msg};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Phase {
    #[default]
    Idle,
    /// Asking the group's call hosts ("Finding a call host…").
    Starting,
    Connecting,
    Active,
    Reconnecting,
    /// My part ended; the screen holds the reason for a moment.
    Ended,
    Failed,
}

/// How long the "started a call" banner stays before it settles (gcall.svelte.ts `GCALL_MS.bannerHold`).
pub(super) const BANNER_HOLD: Duration = Duration::from_millis(9000);

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Announce {
    pub gid: String,
    pub call_id: String,
    pub by: String,
    pub banner_on: bool,
    pub dismissed: bool,
    /// Rings like an incoming call (up to `group_call_ring_ms`): Join / Not now. Then the quiet banner stays.
    pub ringing: bool,
}

#[derive(Default)]
pub(super) struct State {
    pub gid: Option<String>,
    pub call_id: Option<String>,
    pub phase: Phase,
    /// Everyone in the call (DIDs, me included), in roster order.
    pub parts: Vec<String>,
    /// When the call started (its `started_at`, else when I joined).
    pub since: Option<Instant>,
    pub muted: bool,
    pub minimized: bool,
    pub about: bool,
    /// The holder of the call host device (from `Joined`), and whether it is this computer.
    pub host: Option<String>,
    pub hosting: bool,
    /// Ended / Failed: what to say ("Call ended", "No call host available", ...).
    pub reason: String,
    /// A note from the host (battery, heat, metered data), dismissable.
    pub note: Option<String>,
    /// Calls running in my groups, by group (from `group_calls`).
    pub live: HashMap<String, p2pcore::GroupCall>,
    ended_at: Option<Instant>,
    /// GroupCallAnnounce.svelte: "Sam started a call in Weekend hike". A quiet banner that settles after
    /// `BANNER_HOLD` (the call stays in the conversation and the Groups list). A group call someone else starts rings
    /// first (`Announce::ringing`) when I am on no line.
    pub announce: Option<Announce>,
    /// Screenshots: a fixed call length (a timer changing in the captured frame isn't drawn yet).
    pub frozen: Option<u64>,
}

#[derive(Debug, Clone)]
pub(super) enum Gm {
    /// The group header's call button: start a call, join the one running, or return to mine.
    Call(String),
    Started(Result<String, String>),
    Joined(Result<(), String>),
    Leave,
    Mute,
    Minimize,
    Restore,
    About(bool),
    NoteOk,
    Ev(GroupCallEvent),
    /// The banner's hold ran out (only for the call it was raised for).
    Settle(String),
    DismissAnnounce,
    /// "Not now" on the ringing card: the ring stops, the quiet banner stays.
    NotNow,
    /// The ring time ran out (only for the call it was raised for).
    RingEnd(String),
    /// The ended screen's hold ran out (only for the end it was started for).
    Close(Option<Instant>),
}

impl State {
    pub fn live(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }
    /// gcall.svelte.ts `statusText`.
    pub fn status(&self) -> String {
        match self.phase {
            Phase::Starting => "Finding a call host\u{2026}".into(),
            Phase::Connecting => "Connecting\u{2026}".into(),
            Phase::Reconnecting => "Reconnecting\u{2026}".into(),
            Phase::Ended | Phase::Failed => self.reason.clone(),
            _ => String::new(),
        }
    }
    pub fn elapsed(&self) -> u64 {
        self.frozen.unwrap_or_else(|| self.since.map_or(0, |s| s.elapsed().as_secs()))
    }
}

impl App {
    fn gcall_refresh_live(&mut self, gid: &str) {
        match self.node.group_calls(gid.to_string()) {
            Ok(v) => match v.into_iter().max_by_key(|c| c.started_at) {
                Some(c) => {
                    self.gcall.live.insert(gid.to_string(), c);
                }
                None => {
                    self.gcall.live.remove(gid);
                }
            },
            Err(_) => {
                self.gcall.live.remove(gid);
            }
        }
    }

    /// The ring ends (joined, Not now, ended, the ring time): the quiet banner / pill stays.
    fn gcall_stop_ring(&mut self) {
        if let Some(a) = self.gcall.announce.as_mut().filter(|a| a.ringing) {
            a.ringing = false;
            self.ctl.stop_ring();
        }
    }

    fn gcall_end(&mut self, failed: bool, reason: String, hold: u64) -> Task<Msg> {
        self.gcall.phase = if failed { Phase::Failed } else { Phase::Ended };
        self.gcall.reason = reason;
        self.gcall.about = false;
        let at = Instant::now();
        self.gcall.ended_at = Some(at);
        if hold == 0 {
            return self.update(Msg::GCall(Gm::Close(Some(at))));
        }
        Task::perform(tokio::time::sleep(Duration::from_millis(hold)), move |_| Msg::GCall(Gm::Close(Some(at))))
    }

    pub(super) fn gcall_update(&mut self, m: Gm) -> Task<Msg> {
        match m {
            Gm::Call(gid) => {
                if self.gcall.announce.as_ref().is_some_and(|a| a.gid == gid) { self.gcall_stop_ring(); self.gcall.announce = None; }
                if self.gcall.gid.as_deref() == Some(gid.as_str()) && self.gcall.live() {
                    self.gcall.minimized = false;
                    return Task::none();
                }
                if self.gcall.live() || self.call.is_some() {
                    // One line at a time: a group call and a 1:1 call exclude each other.
                    self.notice = Some("You\u{2019}re already in a call.".into());
                    return Task::none();
                }
                self.gcall_refresh_live(&gid);
                self.gcall = State { live: std::mem::take(&mut self.gcall.live), ..State::default() };
                self.gcall.gid = Some(gid.clone());
                let node = self.node.clone();
                if let Some(c) = self.gcall.live.get(&gid).cloned() {
                    self.gcall.phase = Phase::Connecting;
                    self.gcall.call_id = Some(c.call_id.clone());
                    // `started_at` is unix ms.
                    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                    self.gcall.since = Instant::now().checked_sub(Duration::from_millis(now_ms.saturating_sub(c.started_at)));
                    return blocking(move || node.group_call_join(gid, c.call_id).map_err(|e| e.to_string()), |r| Msg::GCall(Gm::Joined(r)));
                }
                self.gcall.phase = Phase::Starting;
                return blocking(move || node.group_call_start(gid).map_err(|e| e.to_string()), |r| Msg::GCall(Gm::Started(r)));
            }
            Gm::Started(r) => match r {
                Ok(id) => {
                    if self.gcall.phase == Phase::Starting {
                        self.gcall.call_id = Some(id);
                        self.gcall.phase = Phase::Connecting;
                    }
                }
                Err(e) => return self.gcall_end(true, start_failure(&e), 2600),
            },
            Gm::Joined(r) => {
                if let Err(e) = r {
                    return self.gcall_end(true, start_failure(&e), 2600);
                }
            }
            Gm::Leave => {
                self.node.group_call_leave();
                // My own leaving goes at once (call.svelte.ts #finish: no hold for a local end).
                return self.gcall_end(false, "Call ended".into(), 0);
            }
            Gm::Mute => {
                self.gcall.muted = !self.gcall.muted;
                self.node.group_call_set_muted(self.gcall.muted);
            }
            Gm::Minimize => {
                if matches!(self.gcall.phase, Phase::Starting | Phase::Connecting | Phase::Active | Phase::Reconnecting) {
                    self.gcall.minimized = true;
                    self.gcall.about = false;
                }
            }
            Gm::Restore => self.gcall.minimized = false,
            Gm::About(on) => self.gcall.about = on,
            Gm::NoteOk => self.gcall.note = None,
            Gm::Close(at) => {
                if self.gcall.ended_at == at && matches!(self.gcall.phase, Phase::Ended | Phase::Failed) {
                    let live = std::mem::take(&mut self.gcall.live);
                    self.gcall = State { live, ..State::default() };
                }
            }
            Gm::Ev(e) => return self.gcall_event(e),
            Gm::Settle(call_id) => {
                if let Some(a) = self.gcall.announce.as_mut().filter(|a| a.call_id == call_id) { a.banner_on = false; }
            }
            Gm::DismissAnnounce | Gm::NotNow => {
                self.gcall_stop_ring();
                if let Some(a) = self.gcall.announce.as_mut() { a.dismissed = true; }
            }
            Gm::RingEnd(call_id) => {
                if self.gcall.announce.as_ref().is_some_and(|a| a.call_id == call_id) { self.gcall_stop_ring(); }
            }
        }
        Task::none()
    }

    fn gcall_event(&mut self, e: GroupCallEvent) -> Task<Msg> {
        let mine = self.gcall.gid.as_deref() == Some(e.group_id.as_str()) && self.gcall.live();
        // The core logs the call before it tells us; the thread and the Calls tab read it back.
        if !self.demo && matches!(e.kind, GroupCallEventKind::Ended | GroupCallEventKind::Left) {
            self.group_log = self.node.group_call_history(None).unwrap_or_default();
        }
        // My own leaving (or the end) may leave the call running for others, or over for everyone: re-read, so Join never lingers.
        if e.kind == GroupCallEventKind::Left && !e.group_id.is_empty() {
            self.gcall_refresh_live(&e.group_id);
        }
        match e.kind {
            GroupCallEventKind::Announced => {
                let fresh = !self.gcall.live.get(&e.group_id).is_some_and(|c| c.call_id == e.call_id);
                self.gcall_refresh_live(&e.group_id);
                // Someone else's call, in a group I'm not already on the line in: raise the banner.
                let theirs = self.gcall.live.get(&e.group_id).is_some_and(|c| c.call_id == e.call_id && !c.joined && !self.is_me(&c.starter_did));
                if fresh && theirs && !(self.gcall.live() && mine) {
                    // It rings when the core agrees (not blocked, no group call here), no 1:1 call is on, and Calls alerts are on.
                    let rings = self.call.is_none() && !self.gcall.live() && crate::audio::RING_CALLS.load(std::sync::atomic::Ordering::Relaxed)
                        && self.node.group_call_rings(e.group_id.clone(), e.call_id.clone());
                    self.gcall_stop_ring();
                    self.gcall.announce = Some(Announce { gid: e.group_id.clone(), call_id: e.call_id.clone(), by: e.detail.clone(), banner_on: true, dismissed: false, ringing: rings });
                    let id = e.call_id.clone();
                    let ring_ms = if rings { self.node.group_call_ring_ms() } else { 0 };
                    let mut tasks = vec![Task::perform(tokio::time::sleep(BANNER_HOLD + Duration::from_millis(ring_ms)), { let id = id.clone(); move |_| Msg::GCall(Gm::Settle(id.clone())) })];
                    if rings {
                        self.ctl.ring_group();
                        let who = self.gcall_name(&e.detail);
                        let group = self.live.list.iter().find(|s| s.id == e.group_id).map(|s| s.name.clone()).unwrap_or_else(|| "Group".into());
                        if !self.focused {
                            super::notify(&group, &format!("{} is calling the group", who.split_whitespace().next().unwrap_or(&who)), super::Target::Show);
                        }
                        tasks.push(Task::perform(tokio::time::sleep(Duration::from_millis(ring_ms)), move |_| Msg::GCall(Gm::RingEnd(id.clone()))));
                    }
                    return Task::batch(tasks);
                }
            }
            GroupCallEventKind::Ended => {
                self.gcall.live.remove(&e.group_id);
                if self.gcall.announce.as_ref().is_some_and(|a| a.gid == e.group_id) { self.gcall_stop_ring(); self.gcall.announce = None; }
                if mine && self.gcall.call_id.as_deref() == Some(e.call_id.as_str()) {
                    return self.gcall_end(false, "Call ended".into(), 900);
                }
            }
            GroupCallEventKind::StartFailed if mine => {
                return self.gcall_end(true, start_failure(&e.detail), 2600);
            }
            GroupCallEventKind::Joined if mine => {
                self.gcall.call_id = Some(e.call_id.clone());
                self.gcall.phase = Phase::Active;
                self.gcall.since.get_or_insert_with(Instant::now);
                self.gcall.hosting = e.detail.is_empty() || self.is_me(&e.detail);
                self.gcall.host = (!e.detail.is_empty()).then(|| e.detail.clone());
                self.gcall_refresh_live(&e.group_id);
            }
            GroupCallEventKind::RosterChanged if mine => self.gcall.parts = e.participants,
            GroupCallEventKind::Reconnecting if mine => self.gcall.phase = Phase::Reconnecting,
            GroupCallEventKind::Left if mine && !matches!(self.gcall.phase, Phase::Ended | Phase::Failed) => {
                let host_lost = e.detail.contains("HostLost") || e.detail.contains("Lost");
                return self.gcall_end(false, if host_lost { "Call ended \u{2014} host lost".into() } else { "Call ended".into() }, 900);
            }
            GroupCallEventKind::HostingStarted => self.gcall.hosting = true,
            GroupCallEventKind::HostingEnded => self.gcall.hosting = false,
            GroupCallEventKind::HostingLimited if mine && self.gcall.hosting => {
                self.gcall.note = Some("Battery is low or the computer is hot. Hosting is limited: only one person is heard at a time, and no one new can join.".into());
            }
            GroupCallEventKind::HostingUnlimited => self.gcall.note = None,
            _ => {}
        }
        Task::none()
    }
}

/// gcall.svelte.ts `failed` wording: no host vs. anything else.
fn start_failure(detail: &str) -> String {
    let d = detail.to_lowercase();
    if d.contains("host") { "No call host available".into() } else { "Couldn\u{2019}t connect".into() }
}
