//! Chat: state, updates and the bridge from core events. The screens are in `chat_view.rs`.
//! Every call into the chat API goes through `Source`, so a test-hooks build can swap in the
//! in-memory `chat_fake`. An erroring API (the real one, until it is filled in) leaves the
//! lists empty; only a send, edit or save the user asked for reports its error.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::text_editor;
use iced::{clipboard, Task};
use p2pcore::{Chat, Contact, DayPage, Error, Message as ChatMsg, Node, TransferState};

use crate::voice::{Player, Recorder};
use super::{blocking, notify, s, App, Msg, Screen};
use crate::ChatEv;

#[cfg(feature = "test-hooks")]
#[path = "chat_fake.rs"]
pub(super) mod chat_fake;

#[derive(PartialEq, Clone, Copy, Debug)]
pub(super) enum SideTab {
    Chats,
    Calls,
    Contacts,
}

/// Where chat calls go: the node, or (test-hooks) an in-memory fake.
#[derive(Clone)]
pub(super) enum Source {
    Node(Arc<Node>),
    #[cfg(feature = "test-hooks")]
    Fake(Arc<chat_fake::Fake>),
}

macro_rules! route {
    ($self:ident.$m:ident($($a:expr),*)) => {
        match $self {
            Source::Node(n) => n.$m($($a),*),
            #[cfg(feature = "test-hooks")]
            Source::Fake(f) => f.$m($($a),*),
        }
    };
}

impl Source {
    pub fn chats(&self, contacts: Vec<Contact>) -> Result<Vec<Chat>, Error> {
        match self {
            Source::Node(n) => {
                let _ = contacts;
                n.chats()
            }
            #[cfg(feature = "test-hooks")]
            Source::Fake(f) => f.chats(contacts),
        }
    }
    pub fn chat_day(&self, peer: String, day: Option<String>) -> Result<DayPage, Error> {
        route!(self.chat_day(peer, day))
    }
    pub fn fetch_older_history(&self, peer: String, before: String) -> Result<u32, Error> {
        route!(self.fetch_older_history(peer, before))
    }
    pub fn send_text(&self, peer: String, text: String, reply: Option<String>) -> Result<ChatMsg, Error> {
        route!(self.send_text(peer, text, reply))
    }
    pub fn edit_message(&self, peer: String, id: String, text: String) -> Result<ChatMsg, Error> {
        route!(self.edit_message(peer, id, text))
    }
    pub fn delete_message(&self, peer: String, id: String) -> Result<ChatMsg, Error> {
        route!(self.delete_message(peer, id))
    }
    pub fn mark_read(&self, peer: String) -> Result<(), Error> {
        route!(self.mark_read(peer))
    }
    pub fn send_file(&self, peer: String, path: String, mime: String) -> Result<ChatMsg, Error> {
        match self {
            Source::Node(n) => n.send_file(peer, path, mime, None),
            #[cfg(feature = "test-hooks")]
            Source::Fake(f) => f.send_file(peer, path, mime),
        }
    }
    pub fn send_voice(&self, peer: String, path: String, duration_ms: u32, waveform: Vec<u8>) -> Result<ChatMsg, Error> {
        route!(self.send_voice(peer, path, duration_ms, waveform))
    }
    pub fn download_attachment(&self, peer: String, id: String) -> Result<(), Error> {
        route!(self.download_attachment(peer, id))
    }
    pub fn save_attachment(&self, peer: String, id: String, dest: String) -> Result<(), Error> {
        route!(self.save_attachment(peer, id, dest))
    }
}

pub(super) struct ChatState {
    pub tab: SideTab,
    pub chats: Vec<Chat>,
    /// The conversation whose messages are loaded.
    pub peer: Option<String>,
    /// Oldest first.
    pub msgs: Vec<ChatMsg>,
    /// The next older day we hold, if any.
    pub older: Option<String>,
    pub loading: bool,
    pub fetching: bool,
    /// Set when asking the peer for earlier days found nothing.
    pub no_more: bool,
    pub input: text_editor::Content,
    pub reply: Option<String>,
    pub editing: Option<String>,
    pub hover: Option<String>,
    pub menu: Option<String>,
    /// Attachment hash -> (done, total) while downloading.
    pub progress: HashMap<String, (u64, u64)>,
    /// A file is being dragged over the window.
    pub drop_hover: bool,
    /// The contact details are shown instead of the conversation.
    pub info: bool,
    /// A voice message being recorded, and the file it goes to.
    pub rec: Option<(Recorder, PathBuf)>,
    /// The voice message playing (or paused), by message id.
    pub player: Option<(String, Player)>,
    /// Decrypted voice files by message id, for playing again; removed when the app quits.
    pub voice_files: HashMap<String, PathBuf>,
    #[cfg(feature = "test-hooks")]
    pub fake: Option<Arc<chat_fake::Fake>>,
}

impl Default for ChatState {
    fn default() -> Self {
        ChatState {
            tab: SideTab::Chats,
            chats: Vec::new(),
            peer: None,
            msgs: Vec::new(),
            older: None,
            loading: false,
            fetching: false,
            no_more: false,
            input: text_editor::Content::new(),
            reply: None,
            editing: None,
            hover: None,
            menu: None,
            progress: HashMap::new(),
            drop_hover: false,
            info: false,
            rec: None,
            player: None,
            voice_files: HashMap::new(),
            #[cfg(feature = "test-hooks")]
            fake: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Cm {
    Tab(SideTab),
    Chats(Result<Vec<Chat>, String>),
    /// peer, whether the page goes in front of what is loaded
    Day(String, bool, Result<DayPage, String>),
    Edit(text_editor::Action),
    Send,
    /// A message came back from send / edit / delete.
    Done(Result<ChatMsg, String>),
    Hover(Option<String>),
    Menu(String),
    Reply(String),
    StartEdit(String),
    Delete(String),
    Copy(String),
    CancelCompose,
    Attach,
    Dropped(PathBuf),
    DropHover(bool),
    Download(String),
    Save(String),
    Saved(Result<String, String>),
    Older,
    OlderDone(String, String, Result<u32, String>),
    Info(bool),
    RecStart,
    RecSend,
    RecCancel,
    VoiceToggle(String),
    VoiceSeek(String, f32),
    VoiceReady(String, Result<PathBuf, String>),
    Nop,
}

pub(super) fn mime_of(path: &std::path::Path) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "txt" => "text/plain",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
    .into()
}

/// A free file name in `dir` for `name`: "a.pdf", then "a (2).pdf", ...
fn free_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let safe = std::path::Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("attachment");
    let p = std::path::Path::new(safe);
    let stem = p.file_stem().and_then(|x| x.to_str()).unwrap_or("attachment");
    let ext = p.extension().and_then(|x| x.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    let mut out = dir.join(safe);
    let mut n = 2;
    while out.exists() {
        out = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    out
}

impl ChatState {
    fn key(m: &ChatMsg) -> (u64, &str) {
        (m.at, m.id.as_str())
    }

    /// Inserts or replaces by id, keeping (at, id) order. Other conversations are ignored.
    pub fn upsert(&mut self, m: ChatMsg) {
        if self.peer.as_deref() != Some(m.peer_did.as_str()) {
            return;
        }
        if let Some(a) = &m.attachment
            && a.state != TransferState::Downloading
        {
            self.progress.remove(&a.hash);
        }
        if let Some(i) = self.msgs.iter().position(|x| x.id == m.id) {
            self.msgs[i] = m;
        } else {
            let at = self.msgs.partition_point(|x| Self::key(x) < Self::key(&m));
            self.msgs.insert(at, m);
        }
    }

    pub fn find(&self, id: &str) -> Option<&ChatMsg> {
        self.msgs.iter().find(|m| m.id == id)
    }

    fn sort_chats(&mut self) {
        self.chats.sort_by(|a, b| b.last_activity.cmp(&a.last_activity).then_with(|| a.peer_name.cmp(&b.peer_name)));
    }

    fn upsert_chat(&mut self, c: Chat) {
        match self.chats.iter().position(|x| x.peer_did == c.peer_did) {
            Some(i) => self.chats[i] = c,
            None => self.chats.push(c),
        }
        self.sort_chats();
    }

    pub fn unread_total(&self) -> u32 {
        self.chats.iter().map(|c| c.unread).sum()
    }

    fn clear_compose(&mut self) {
        self.input = text_editor::Content::new();
        self.reply = None;
        self.editing = None;
        self.menu = None;
    }
}

/// Scratch space for recordings and decrypted voice files; wiped at start and at quit.
pub(super) fn voice_dir() -> PathBuf {
    let d = std::env::temp_dir().join("tinline-voice");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub(super) fn wipe_voice_dir() {
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("tinline-voice"));
}

/// Widget id of the message field.
pub(super) const COMPOSER_ID: &str = "composer";

impl App {
    pub(super) fn src(&self) -> Source {
        #[cfg(feature = "test-hooks")]
        if let Some(f) = &self.chat.fake {
            return Source::Fake(f.clone());
        }
        Source::Node(self.node.clone())
    }

    /// Chat IO is skipped in demo screens unless they brought a fake source.
    fn chat_io(&self) -> bool {
        #[cfg(feature = "test-hooks")]
        if self.chat.fake.is_some() {
            return true;
        }
        !self.demo
    }

    /// The conversation is on screen (not contact details, settings, a call...).
    pub(super) fn chat_visible(&self) -> bool {
        self.screen == Screen::Home
            && self.sel.is_some()
            && !self.chat.info
            && self.call.is_none()
            && self.ended.is_none()
    }

    pub(super) fn refresh_chats(&self) -> Task<Msg> {
        if !self.chat_io() {
            return Task::none();
        }
        let (src, contacts) = (self.src(), self.contacts.clone());
        blocking(move || src.chats(contacts).map_err(s), |r| Msg::Chat(Cm::Chats(r)))
    }

    fn load_day(&self, peer: String, day: Option<String>, prepend: bool) -> Task<Msg> {
        let src = self.src();
        let p = peer.clone();
        blocking(move || src.chat_day(p, day).map_err(s), move |r| Msg::Chat(Cm::Day(peer.clone(), prepend, r)))
    }

    fn mark_read(&mut self, peer: &str) -> Task<Msg> {
        if let Some(c) = self.chat.chats.iter_mut().find(|c| c.peer_did == peer) {
            if c.unread == 0 {
                return Task::none();
            }
            c.unread = 0;
        }
        if !self.chat_io() {
            return Task::none();
        }
        let (src, p) = (self.src(), peer.to_string());
        blocking(move || src.mark_read(p).map_err(s), |_| Msg::Chat(Cm::Nop))
    }

    /// Opens the conversation with `did` in the main pane.
    pub(super) fn open_chat(&mut self, did: &str) -> Task<Msg> {
        self.chat.info = false;
        if self.chat.peer.as_deref() == Some(did) {
            return self.mark_read(did);
        }
        self.chat.clear_compose();
        self.chat.peer = Some(did.to_string());
        self.chat.msgs.clear();
        self.chat.older = None;
        self.chat.no_more = false;
        self.chat.fetching = false;
        self.chat.progress.clear();
        if !self.chat_io() {
            return Task::none();
        }
        self.chat.loading = true;
        Task::batch([self.load_day(did.to_string(), None, false), self.mark_read(did)])
    }

    pub(super) fn leave_chat(&mut self) {
        self.chat.peer = None;
        self.chat.info = false;
        self.chat.clear_compose();
    }

    /// The name to show for a conversation partner.
    pub(super) fn chat_name(&self, did: &str) -> String {
        match self.contacts.iter().find(|c| c.did == did) {
            Some(c) => Self::display(c),
            None => self.chat.chats.iter().find(|c| c.peer_did == did).map(|c| c.peer_name.clone()).unwrap_or_default(),
        }
    }

    fn send_task(
        &self,
        peer: String,
        f: impl FnOnce(Source, String) -> Result<ChatMsg, Error> + Send + 'static,
    ) -> Task<Msg> {
        let src = self.src();
        blocking(move || f(src, peer).map_err(s), |r| Msg::Chat(Cm::Done(r)))
    }

    pub(super) fn update_chat(&mut self, m: Cm) -> Task<Msg> {
        match m {
            Cm::Tab(t) => {
                self.chat.tab = t;
                if t == SideTab::Chats {
                    return self.refresh_chats();
                }
            }
            Cm::Chats(r) => {
                // An erroring API is the empty state, not a banner.
                self.chat.chats = r.unwrap_or_default();
                self.chat.sort_chats();
            }
            Cm::Day(peer, prepend, r) => {
                if self.chat.peer.as_deref() != Some(peer.as_str()) {
                    return Task::none();
                }
                self.chat.loading = false;
                self.chat.fetching = false;
                let page = r.unwrap_or(DayPage { day: String::new(), messages: Vec::new(), older_day: None });
                self.chat.older = page.older_day;
                if prepend {
                    let mut merged = page.messages;
                    merged.retain(|m| self.chat.find(&m.id).is_none());
                    merged.append(&mut self.chat.msgs);
                    self.chat.msgs = merged;
                } else {
                    // Anything that arrived while the page loaded stays.
                    let mut late: Vec<ChatMsg> = std::mem::take(&mut self.chat.msgs)
                        .into_iter()
                        .filter(|m| !page.messages.iter().any(|p| p.id == m.id))
                        .collect();
                    self.chat.msgs = page.messages;
                    self.chat.msgs.append(&mut late);
                    self.chat.msgs.sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
                }
            }
            Cm::Edit(a) => self.chat.input.perform(a),
            Cm::Send => {
                let Some(peer) = self.chat.peer.clone() else { return Task::none() };
                let text = self.chat.input.text().trim_end().to_string();
                if text.trim().is_empty() || !self.chat_io() {
                    return Task::none();
                }
                let (reply, editing) = (self.chat.reply.take(), self.chat.editing.take());
                self.chat.input = text_editor::Content::new();
                self.chat.menu = None;
                return self.send_task(peer, move |src, peer| match editing {
                    Some(id) => src.edit_message(peer, id, text),
                    None => src.send_text(peer, text, reply),
                });
            }
            Cm::Done(r) => match r {
                Ok(m) => self.chat.upsert(m),
                Err(e) => self.notice = Some(format!("Could not send: {e}")),
            },
            Cm::Hover(h) => self.chat.hover = h,
            Cm::Menu(id) => self.chat.menu = if self.chat.menu.as_ref() == Some(&id) { None } else { Some(id) },
            Cm::Reply(id) => {
                self.chat.reply = Some(id);
                self.chat.editing = None;
                self.chat.menu = None;
                return iced::widget::operation::focus(COMPOSER_ID);
            }
            Cm::StartEdit(id) => {
                if let Some(m) = self.chat.find(&id).filter(|m| m.outgoing && !m.deleted) {
                    self.chat.input = text_editor::Content::with_text(&m.text);
                    self.chat.editing = Some(id);
                    self.chat.reply = None;
                    self.chat.menu = None;
                    return iced::widget::operation::focus(COMPOSER_ID);
                }
            }
            Cm::CancelCompose => {
                if self.chat.editing.is_some() {
                    self.chat.input = text_editor::Content::new();
                }
                self.chat.editing = None;
                self.chat.reply = None;
            }
            Cm::Delete(id) => {
                self.chat.menu = None;
                let Some(peer) = self.chat.peer.clone() else { return Task::none() };
                if self.chat.editing.as_ref() == Some(&id) {
                    self.chat.clear_compose();
                }
                return self.send_task(peer, move |src, peer| src.delete_message(peer, id));
            }
            Cm::Copy(text) => {
                self.chat.menu = None;
                return clipboard::write(text);
            }
            Cm::Attach => self.notice = Some("Drag files into this window to send them.".into()),
            Cm::DropHover(on) => self.chat.drop_hover = on && self.chat_visible(),
            Cm::Dropped(path) => {
                self.chat.drop_hover = false;
                let Some(peer) = self.chat.peer.clone().filter(|_| self.chat_visible() && self.chat_io()) else {
                    return Task::none();
                };
                if !path.is_file() {
                    self.notice = Some("Only files can be sent, not folders.".into());
                    return Task::none();
                }
                let mime = mime_of(&path);
                let path = path.to_string_lossy().into_owned();
                return self.send_task(peer, move |src, peer| src.send_file(peer, path, mime));
            }
            Cm::Download(id) => {
                let Some(peer) = self.chat.peer.clone() else { return Task::none() };
                if let Some(a) = self.chat.msgs.iter_mut().find(|m| m.id == id).and_then(|m| m.attachment.as_mut()) {
                    a.state = TransferState::Downloading;
                }
                let src = self.src();
                return blocking(
                    move || src.download_attachment(peer, id).map_err(s),
                    |r| match r {
                        Ok(()) => Msg::Chat(Cm::Nop),
                        Err(e) => Msg::Chat(Cm::Saved(Err(e))),
                    },
                );
            }
            Cm::Save(id) => {
                let Some(peer) = self.chat.peer.clone() else { return Task::none() };
                let Some(name) = self.chat.find(&id).and_then(|m| m.attachment.as_ref()).map(|a| a.name.clone()) else {
                    return Task::none();
                };
                // No file dialog crate in the app: files land in Downloads, never overwriting.
                let dir = directories::UserDirs::new()
                    .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
                    .unwrap_or_else(std::env::temp_dir);
                let shown = free_path(&dir, &name).to_string_lossy().into_owned();
                let src = self.src();
                return blocking(
                    move || src.save_attachment(peer, id, shown.clone()).map(|_| shown).map_err(s),
                    |r| Msg::Chat(Cm::Saved(r)),
                );
            }
            Cm::Saved(r) => match r {
                Ok(p) => self.notice = Some(format!("Saved to {p}")),
                Err(e) => self.notice = Some(format!("Could not save: {e}")),
            },
            Cm::Older => {
                let Some(peer) = self.chat.peer.clone() else { return Task::none() };
                if self.chat.fetching {
                    return Task::none();
                }
                self.chat.fetching = true;
                if let Some(day) = self.chat.older.clone() {
                    return self.load_day(peer, Some(day), true);
                }
                // Nothing older here: ask the peer for the days before our first message.
                let first = self.chat.msgs.first().and_then(|m| chrono::DateTime::from_timestamp_millis(m.at as i64));
                let Some(first) = first else {
                    self.chat.fetching = false;
                    return Task::none();
                };
                let before = first.format("%Y-%m-%d").to_string();
                let (src, p, b) = (self.src(), peer.clone(), before.clone());
                return blocking(
                    move || src.fetch_older_history(p, b).map_err(s),
                    move |r| Msg::Chat(Cm::OlderDone(peer.clone(), before.clone(), r)),
                );
            }
            Cm::OlderDone(peer, before, r) => {
                if self.chat.peer.as_deref() != Some(peer.as_str()) {
                    return Task::none();
                }
                match r {
                    Ok(n) if n > 0 => return self.load_day(peer, Some(before), true),
                    _ => {
                        self.chat.fetching = false;
                        self.chat.no_more = true;
                    }
                }
            }
            Cm::Info(on) => self.chat.info = on,
            Cm::RecStart => {
                if self.chat.peer.is_none() || self.chat.rec.is_some() || !self.chat_io() {
                    return Task::none();
                }
                self.chat.player = None;
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                let path = voice_dir().join(format!("rec-{stamp}.ogg"));
                match Recorder::start(&path) {
                    Ok(r) => self.chat.rec = Some((r, path)),
                    Err(e) => self.notice = Some(format!("Could not record: {e}")),
                }
            }
            Cm::RecCancel => {
                if let Some((r, _)) = self.chat.rec.take() {
                    r.cancel();
                }
            }
            Cm::RecSend => {
                let (Some((rec, path)), Some(peer)) = (self.chat.rec.take(), self.chat.peer.clone()) else {
                    return Task::none();
                };
                match rec.finish() {
                    Ok(Some(info)) => {
                        let p = path.to_string_lossy().into_owned();
                        return self.send_task(peer, move |src, peer| {
                            let r = src.send_voice(peer, p.clone(), info.duration_ms, info.waveform);
                            let _ = std::fs::remove_file(p);
                            r
                        });
                    }
                    Ok(None) => self.notice = Some("Too short \u{2014} hold on a little longer.".into()),
                    Err(e) => self.notice = Some(format!("Could not record: {e}")),
                }
            }
            Cm::VoiceToggle(id) => {
                if let Some((pid, pl)) = &self.chat.player
                    && *pid == id
                {
                    if pl.finished() {
                        self.chat.player = None;
                    } else {
                        if pl.is_paused() { pl.resume() } else { pl.pause() }
                        return Task::none();
                    }
                }
                self.chat.player = None;
                if let Some(p) = self.chat.voice_files.get(&id).filter(|p| p.exists()).cloned() {
                    return self.update_chat(Cm::VoiceReady(id, Ok(p)));
                }
                let Some(peer) = self.chat.peer.clone() else { return Task::none() };
                let dest = voice_dir().join(format!("play-{}.ogg", id.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>()));
                let src = self.src();
                let (i, d) = (id.clone(), dest.clone());
                return blocking(
                    move || src.save_attachment(peer, i, d.to_string_lossy().into_owned()).map(|_| d).map_err(s),
                    move |r| Msg::Chat(Cm::VoiceReady(id.clone(), r)),
                );
            }
            Cm::VoiceReady(id, r) => match r.and_then(|p| Player::play(&p).map(|pl| (p, pl))) {
                Ok((p, pl)) => {
                    self.chat.voice_files.insert(id.clone(), p);
                    self.chat.player = Some((id, pl));
                }
                Err(e) => self.notice = Some(format!("Could not play: {e}")),
            },
            Cm::VoiceSeek(id, f) => {
                if let Some((pid, pl)) = &self.chat.player
                    && *pid == id
                {
                    pl.seek((f * pl.duration_ms() as f32) as u32);
                } else {
                    // Seeking a bubble that is not playing starts it.
                    return self.update_chat(Cm::VoiceToggle(id));
                }
            }
            Cm::Nop => {
                // Ticks at 100 ms while recording or playing redraw the timer and the wave.
                if self.chat.player.as_ref().is_some_and(|(_, p)| p.finished()) {
                    self.chat.player = None;
                }
                if self.chat.rec.as_ref().is_some_and(|(r, _)| r.elapsed_ms() >= crate::voice::MAX_MS) {
                    return self.update_chat(Cm::RecSend);
                }
            }
        }
        Task::none()
    }

    /// Core chat events, already on the UI loop.
    pub(super) fn on_chat_event(&mut self, ev: ChatEv) -> Task<Msg> {
        match ev {
            ChatEv::Added(m) => {
                let (peer, incoming) = (m.peer_did.clone(), !m.outgoing);
                let open = self.chat_visible() && self.chat.peer.as_deref() == Some(peer.as_str());
                if incoming && !(open && self.win.is_some()) {
                    notify(&self.chat_name(&peer), &preview_of(&m));
                }
                self.chat.upsert(m);
                if incoming && open {
                    return self.mark_read(&peer);
                }
            }
            ChatEv::Changed(m) => self.chat.upsert(m),
            ChatEv::Chat(c) => self.chat.upsert_chat(c),
            ChatEv::Delivery(peer, id, d) => {
                if self.chat.peer.as_deref() == Some(peer.as_str())
                    && let Some(m) = self.chat.msgs.iter_mut().find(|m| m.id == id)
                {
                    m.delivery = d;
                }
            }
            ChatEv::Progress { hash, done, total, outgoing } => {
                if !outgoing {
                    self.chat.progress.insert(hash, (done, total));
                }
            }
        }
        Task::none()
    }
}

/// What a notification or a list row says about a message.
pub(super) fn preview_of(m: &ChatMsg) -> String {
    if m.deleted {
        return "Message deleted".into();
    }
    let t: String = m.text.chars().take(120).collect();
    match (&m.attachment, t.is_empty()) {
        (_, false) => t,
        (Some(a), true) if a.kind == p2pcore::AttachmentKind::Voice => "Voice message".into(),
        (Some(a), true) => a.name.clone(),
        (None, true) => String::new(),
    }
}
