//! Chat: state, updates and the bridge from core events. The screens are in `chat_view.rs`.
//! Every call into the chat API goes through `Source`, so a test-hooks build can swap in the
//! in-memory `chat_fake`. An erroring API (the real one, until it is filled in) leaves the
//! lists empty; only a send, edit or save the user asked for reports its error.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::text_editor;
use iced::{clipboard, Task};
use p2pcore::{Attachment, AttachmentKind, Chat, Contact, DayPage, Error, Message as ChatMsg, MessageReceiptStatus, Node, TransferState};

use crate::app::Target;
use crate::media::{self, Class, Pixels};

use crate::voice::{Player, Recorder};
use super::{blocking, notify, s, App, Msg, Screen};
use crate::ChatEv;
#[path = "compose_state.rs"]
mod compose_state;

#[cfg(feature = "test-hooks")]
#[path = "chat_fake.rs"]
pub(super) mod chat_fake;

#[derive(PartialEq, Clone, Copy, Debug)]
pub(super) enum SideTab {
    Chats,
    /// Group chat on the core (G1), always shown.
    Groups,
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
    /// Recipient-signed status (core `ReceiptTick` + proofs) of one message.
    pub fn chat_message_receipts(&self, peer: String, id: String) -> Result<MessageReceiptStatus, Error> {
        route!(self.chat_message_receipts(peer, id))
    }
    pub fn chat_day_receipts(&self, peer: String, day: String) -> Result<Vec<MessageReceiptStatus>, Error> {
        route!(self.chat_day_receipts(peer, day))
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
    pub fn contact_online(&self, peer: String) -> bool {
        route!(self.contact_online(peer))
    }
    pub fn watch_presence(&self, peer: String) {
        route!(self.watch_presence(peer))
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
    pub fn cancel_download(&self, peer: String, id: String) -> Result<(), Error> {
        route!(self.cancel_download(peer, id))
    }
    pub fn save_attachment(&self, peer: String, id: String, dest: String) -> Result<(), Error> {
        route!(self.save_attachment(peer, id, dest))
    }
}

pub(super) struct ChatState {
    /// Invalidates in-flight text submissions when the current account changes.
    pub account_epoch: u64,
    pub tab: SideTab,
    pub chats: Vec<Chat>,
    /// The conversation whose messages are loaded.
    pub peer: Option<String>,
    /// Oldest first.
    pub msgs: Vec<ChatMsg>,
    /// The core's receipt status per loaded message id (the bubble mark and its provenance).
    /// Missing = not read from the core yet: the mark's slot stays empty, never "delivered".
    pub receipts: HashMap<String, MessageReceiptStatus>,
    /// The tick state last shown per message, and the one-shot transitions running (`ticks.rs`).
    /// A message seen for the first time (page load, scroll-in) never animates.
    pub tick_vis: HashMap<String, crate::ticks::Vis>,
    pub tick_anims: HashMap<String, crate::ticks::Anim>,
    /// The chat list's mark per conversation whose last message is mine (core `chat_list_marks`):
    /// `(group, conversation id)` -> (message id, tick). Missing = not read yet (slot stays empty).
    pub marks: HashMap<(bool, String), (String, crate::ticks::Vis)>,
    /// The next older day we hold, if any.
    pub older: Option<String>,
    pub loading: bool,
    pub fetching: bool,
    /// Set when asking the peer for earlier days found nothing.
    pub no_more: bool,
    pub input: text_editor::Content,
    compose_memory: compose_state::Memory,
    pub reply: Option<String>,
    pub editing: Option<String>,
    pub hover: Option<String>,
    pub menu: Option<String>,
    /// Attachment hash -> (done, total) while downloading.
    pub progress: HashMap<String, (u64, u64)>,
    /// Contacts reachable right now (the core's ephemeral presence).
    pub online: HashSet<String>,
    /// A file is being dragged over the window.
    pub drop_hover: bool,
    /// The contact details are shown instead of the conversation.
    pub info: bool,
    /// Find in this chat (Ctrl+F): the query while the bar is open.
    pub find: Option<String>,
    /// The current match, counted among the matches oldest first; `usize::MAX` = the newest.
    pub find_cur: usize,
    /// How many unread messages the chat had when it was opened (the "n new messages" divider).
    pub unread_at_open: u32,
    /// "Delete message?" is open for this message.
    pub confirm_delete: Option<String>,
    /// The message a reply quote jumped to, ringed for a moment.
    pub flash: Option<(String, std::time::Instant)>,
    /// A voice message being recorded, and the file it goes to.
    pub rec: Option<(Recorder, PathBuf)>,
    /// The voice message playing (or paused), by message id.
    pub player: Option<(String, Player)>,
    /// Decrypted voice files by message id, for playing again; removed when the app quits.
    pub voice_files: HashMap<String, PathBuf>,
    /// Decoded image thumbnails by attachment hash, so scrolling never decodes twice.
    pub thumbs: HashMap<String, Thumb>,
    /// The image or text overlay.
    pub viewer: Option<Viewer>,
    #[cfg(feature = "test-hooks")]
    pub fake: Option<Arc<chat_fake::Fake>>,
}

#[derive(Clone)]
pub(super) enum Thumb {
    Loading,
    Ready { handle: iced::widget::image::Handle, w: u32, h: u32 },
    /// Not decodable or too large: shown as a file.
    Bad,
}

#[derive(Clone)]
pub(super) enum ViewBody {
    Loading,
    Image { handle: iced::widget::image::Handle },
    Text(String),
    Failed(String),
}

#[derive(Clone)]
pub(super) struct Viewer {
    /// The message it shows.
    pub id: String,
    pub name: String,
    pub body: ViewBody,
}

impl Default for ChatState {
    fn default() -> Self {
        ChatState {
            account_epoch: 0,
            tab: SideTab::Chats,
            chats: Vec::new(),
            peer: None,
            msgs: Vec::new(),
            receipts: HashMap::new(),
            tick_vis: HashMap::new(),
            tick_anims: HashMap::new(),
            marks: HashMap::new(),
            older: None,
            loading: false,
            fetching: false,
            no_more: false,
            input: text_editor::Content::new(),
            compose_memory: compose_state::Memory::default(),
            reply: None,
            editing: None,
            hover: None,
            menu: None,
            progress: HashMap::new(),
            online: HashSet::new(),
            drop_hover: false,
            info: false,
            find: None,
            find_cur: usize::MAX,
            unread_at_open: 0,
            confirm_delete: None,
            flash: None,
            rec: None,
            player: None,
            voice_files: HashMap::new(),
            thumbs: HashMap::new(),
            viewer: None,
            #[cfg(feature = "test-hooks")]
            fake: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Cm {
    Tab(SideTab),
    /// A contact's presence, read when its conversation opened.
    Presence(String, bool),
    Chats(Result<Vec<Chat>, String>),
    /// The chat list's marks (DMs and groups) from the core.
    Marks(Vec<p2pcore::ListMark>),
    /// peer, whether the page goes in front of what is loaded
    Day(String, bool, Result<DayPage, String>),
    /// Receipt statuses read from the core for messages of this peer.
    Receipts(String, Result<Vec<MessageReceiptStatus>, String>),
    Edit(text_editor::Action),
    Send,
    Submitted(String, compose_state::Submission, Result<ChatMsg, String>),
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
    Picked(Option<PathBuf>),
    Dropped(PathBuf),
    DropHover(bool),
    Download(String),
    Cancel(String),
    Save(String),
    Saved(Result<String, String>),
    Older,
    OlderDone(String, String, Result<u32, String>),
    Info(bool),
    /// Open the find bar (Some), type into it, or close it (None).
    Find(Option<String>),
    /// Step to the older (true) or newer match.
    FindStep(bool),
    /// A reply quote: scroll to the quoted message and ring it for a moment.
    Jump(String),
    /// Confirmed in "Delete message?".
    DeleteGo(String),
    DeleteCancel,
    RecStart,
    RecSend,
    RecCancel,
    VoiceToggle(String),
    VoiceSeek(String, f32),
    VoiceReady(String, Result<PathBuf, String>),
    /// Thumbnail of the attachment with this hash.
    Thumb(String, Result<Pixels, String>),
    /// Open the picture in the viewer.
    View(String),
    /// The viewer's picture at full size.
    Viewed(String, Result<Pixels, String>),
    /// Documents: text in the app, anything else in the default app.
    Open(String),
    /// Always the default app.
    OpenExt(String),
    TextShown(Result<String, String>),
    Opened(Result<(), String>),
    CloseView,
    /// -1 / +1: the previous / next picture.
    Step(i32),
    Nop,
}

/// The find field's widget id.
pub(super) const FIND_ID: &str = "chat-find";
/// The message list's scrollable id (find scrolls it to the current match).
pub(super) const LIST_ID: &str = "chat-list";

impl App {
    /// find.svelte / `findInChat`: the loaded messages whose text holds every term of the query, oldest first.
    pub(super) fn find_matches(&self) -> Vec<&ChatMsg> {
        let Some(q) = self.chat.find.as_deref() else { return Vec::new() };
        let terms: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
        if terms.is_empty() {
            return Vec::new();
        }
        self.chat.msgs.iter().filter(|m| !m.deleted).filter(|m| {
            let text = format!("{} {}", m.text, m.attachment.as_ref().map(|a| a.name.as_str()).unwrap_or("")).to_lowercase();
            terms.iter().all(|t| text.contains(t))
        }).collect()
    }

    /// The current match's id, if any.
    pub(super) fn find_current(&self) -> Option<String> {
        let ms = self.find_matches();
        (!ms.is_empty()).then(|| ms[self.chat.find_cur.min(ms.len() - 1)].id.clone())
    }

    /// Scroll so the current match is in view: by its share of the list (rows differ in height, so this is
    /// close, not exact; the match is also ringed).
    fn find_scroll(&self) -> Task<Msg> {
        let Some(id) = self.find_current() else { return Task::none() };
        let n = self.chat.msgs.len().max(2);
        let i = self.chat.msgs.iter().position(|m| m.id == id).unwrap_or(n - 1);
        iced::widget::operation::snap_to(LIST_ID, iced::widget::scrollable::RelativeOffset { x: 0.0, y: i as f32 / (n - 1) as f32 })
    }
}

pub(super) fn mime_of(path: &std::path::Path) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if let Some(m) = media::mime_for_ext(&ext) {
        return m.into();
    }
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

    /// Message ids of the pictures that can be stepped through, oldest first.
    pub fn viewable_images(&self) -> Vec<String> {
        self.msgs
            .iter()
            .filter(|m| !m.deleted)
            .filter(|m| m.attachment.as_ref().is_some_and(|a| matches!(self.thumbs.get(&a.hash), Some(Thumb::Ready { .. }))))
            .map(|m| m.id.clone())
            .collect()
    }

    /// Test-hooks demos: a saved draft for a conversation that is not open.
    #[cfg(feature = "test-hooks")]
    pub(super) fn demo_draft(&mut self, peer: &str, text: &str) {
        self.compose_memory.suspend(peer, text.to_string(), None, false);
    }

    pub(super) fn draft_for(&self, peer: &str) -> String {
        if self.peer.as_deref() == Some(peer) {
            self.compose_memory.edit_stash.clone().unwrap_or_else(|| self.input.text())
        } else { self.compose_memory.restore(peer).text }
    }

    fn clear_compose(&mut self) {
        if let Some(peer) = &self.peer {
            self.compose_memory.suspend(peer, self.input.text(), self.reply.clone(), self.editing.is_some());
        }
        self.input = text_editor::Content::new();
        self.reply = None;
        self.editing = None;
        self.menu = None;
        self.find = None;
    }
}

/// The decrypted copy of a Ready attachment in the private cache (made once).
fn cache_attachment(src: &Source, peer: String, id: String, a: &Attachment, ext: &str) -> Result<PathBuf, String> {
    media::ensure_cached(&a.hash, &a.name, ext, |tmp| src.save_attachment(peer, id, tmp.to_string()).map_err(s))
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

    /// A Ready file in the open conversation: where to read it from, and what it is.
    fn attachment_job(&self, id: &str) -> Option<(Source, String, Attachment)> {
        let peer = self.chat.peer.clone()?;
        let a = self.chat.find(id).filter(|m| !m.deleted)?.attachment.clone()?;
        (a.kind == AttachmentKind::File && a.state == TransferState::Ready).then(|| (self.src(), peer, a))
    }

    /// Starts decoding a thumbnail for each Ready picture that has none yet.
    pub(super) fn ensure_thumbs(&mut self) -> Task<Msg> {
        if !self.chat_io() {
            return Task::none();
        }
        let Some(peer) = self.chat.peer.clone() else { return Task::none() };
        let mut todo = Vec::new();
        for m in self.chat.msgs.iter().filter(|m| !m.deleted) {
            let Some(a) = m.attachment.as_ref().filter(|a| a.kind == AttachmentKind::File && a.state == TransferState::Ready) else {
                continue;
            };
            if self.chat.thumbs.contains_key(&a.hash) {
                continue;
            }
            if let Class::Image(ext) = media::classify(&a.name, &a.mime) {
                todo.push((m.id.clone(), a.clone(), ext));
            }
        }
        let mut tasks = Vec::new();
        for (id, a, ext) in todo {
            // Huge files are not worth decrypting just for a preview.
            if a.size > 100_000_000 {
                self.chat.thumbs.insert(a.hash.clone(), Thumb::Bad);
                continue;
            }
            self.chat.thumbs.insert(a.hash.clone(), Thumb::Loading);
            let (src, p, hash) = (self.src(), peer.clone(), a.hash.clone());
            tasks.push(blocking(
                move || {
                    let path = cache_attachment(&src, p, id, &a, ext)?;
                    media::decode_scaled(&path, media::THUMB_W, media::THUMB_H)
                },
                move |r| Msg::Chat(Cm::Thumb(hash.clone(), r)),
            ));
        }
        Task::batch(tasks)
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
            && (self.call.is_none() || self.call_ui.minimized)
            && self.ended.is_none()
    }

    pub(super) fn refresh_chats(&self) -> Task<Msg> {
        if !self.chat_io() {
            return Task::none();
        }
        let (src, contacts) = (self.src(), self.contacts.clone());
        Task::batch([blocking(move || src.chats(contacts).map_err(s), |r| Msg::Chat(Cm::Chats(r))), self.refresh_marks()])
    }

    /// Re-reads the chat list's marks (my last message's tick per DM and group). Off the UI loop.
    pub(super) fn refresh_marks(&self) -> Task<Msg> {
        if !self.chat_io() {
            return Task::none();
        }
        let node = self.node.clone();
        blocking(move || node.chat_list_marks().unwrap_or_default(), |v| Msg::Chat(Cm::Marks(v)))
    }

    /// The list mark for a conversation whose last message (`last_id`) is mine: `Some(None)` keeps
    /// the slot (nothing read yet, so nothing moves when it arrives), `None` = no slot.
    pub(super) fn list_mark(&self, group: bool, id: &str, last_id: Option<&str>) -> Option<Option<crate::ticks::Vis>> {
        let last_id = last_id?;
        Some(self.chat.marks.get(&(group, id.to_string())).filter(|(m, _)| m == last_id).map(|(_, v)| *v))
    }

    fn load_day(&self, peer: String, day: Option<String>, prepend: bool) -> Task<Msg> {
        let src = self.src();
        let p = peer.clone();
        blocking(move || src.chat_day(p, day).map_err(s), move |r| Msg::Chat(Cm::Day(peer.clone(), prepend, r)))
    }

    fn load_day_receipts(&self, peer: String, day: String) -> Task<Msg> {
        if day.is_empty() || !self.chat_io() {
            return Task::none();
        }
        let (src, p) = (self.src(), peer.clone());
        blocking(move || src.chat_day_receipts(p, day).map_err(s), move |r| Msg::Chat(Cm::Receipts(peer.clone(), r)))
    }

    /// Re-reads one message's receipt status (after it was sent, edited, changed or ticked).
    pub(super) fn load_receipt(&self, peer: &str, id: &str) -> Task<Msg> {
        if self.chat.peer.as_deref() != Some(peer) || !self.chat_io() {
            return Task::none();
        }
        let (src, p, i, peer) = (self.src(), peer.to_string(), id.to_string(), peer.to_string());
        blocking(move || src.chat_message_receipts(p, i).map(|r| vec![r]).map_err(s), move |r| Msg::Chat(Cm::Receipts(peer.clone(), r)))
    }

    pub(crate) fn mark_read(&mut self, peer: &str) -> Task<Msg> {
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
        self.chat.unread_at_open = self.chat.chats.iter().find(|c| c.peer_did == did).map_or(0, |c| c.unread);
        self.chat.peer = Some(did.to_string());
        let draft = self.chat.compose_memory.restore(did);
        self.chat.input = text_editor::Content::with_text(&draft.text);
        self.chat.reply = draft.reply;
        self.chat.msgs.clear();
        self.chat.receipts.clear();
        self.chat.tick_vis.clear();
        self.chat.tick_anims.clear();
        self.chat.older = None;
        self.chat.no_more = false;
        self.chat.fetching = false;
        self.chat.progress.clear();
        self.chat.thumbs.clear();
        self.chat.viewer = None;
        if !self.chat_io() {
            return Task::none();
        }
        self.chat.loading = true;
        if self.is_me(did) {
            return Task::batch([self.load_day(did.to_string(), None, false), self.mark_read(did)]);
        }
        let (src, p) = (self.src(), did.to_string());
        let presence = blocking(
            move || {
                src.watch_presence(p.clone());
                (p.clone(), src.contact_online(p))
            },
            |(p, on)| Msg::Chat(Cm::Presence(p, on)),
        );
        Task::batch([self.load_day(did.to_string(), None, false), self.mark_read(did), presence])
    }

    pub(super) fn leave_chat(&mut self) {
        self.chat.clear_compose();
        self.chat.peer = None;
        self.chat.info = false;
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
        let t = self.update_chat_inner(m);
        self.sync_ticks();
        Task::batch([t, self.ensure_thumbs()])
    }

    /// What my message's tick shows: the core's `ReceiptTick` (Delivered/Read only from signed receipts),
    /// or not delivered (a block). None = no mark (incoming, deleted, notes to self) or not read yet.
    pub(super) fn tick_vis_of(&self, m: &ChatMsg) -> Option<crate::ticks::Vis> {
        if !m.outgoing || m.deleted {
            return None;
        }
        let tick = self.chat.receipts.get(&m.id).map(|r| r.tick);
        p2pcore::tick_vis(tick, self.blocks.not_delivered(&m.peer_did, &m.id))
    }

    /// Starts a tick transition for every loaded message whose state changed since it was last shown.
    pub(super) fn sync_ticks(&mut self) {
        let now = std::time::Instant::now();
        let mut next = HashMap::new();
        for m in &self.chat.msgs {
            if let Some(v) = self.tick_vis_of(m) {
                next.insert(m.id.clone(), v);
            }
        }
        for (id, v) in &next {
            if let Some(old) = self.chat.tick_vis.get(id)
                && old != v
                && !crate::motion::reduced()
            {
                self.chat.tick_anims.insert(id.clone(), crate::ticks::Anim { from: *old, start: now });
                self.fx.now = now;
            }
        }
        self.chat.tick_anims.retain(|id, a| next.get(id).is_some_and(|v| crate::ticks::running(a, *v, now)));
        self.chat.tick_vis = next;
    }

    /// A tick transition is playing (keeps the frame clock running).
    pub(super) fn ticks_running(&self) -> bool {
        let now = std::time::Instant::now();
        self.chat.tick_anims.iter().any(|(id, a)| self.chat.tick_vis.get(id).is_some_and(|v| crate::ticks::running(a, *v, now)))
    }

    fn update_chat_inner(&mut self, m: Cm) -> Task<Msg> {
        match m {
            Cm::Presence(peer, on) => return self.on_chat_event(ChatEv::Presence(peer, on)),
            Cm::Tab(t) => {
                self.chat.tab = t;
                if t == SideTab::Chats {
                    return self.refresh_chats();
                }
                if t == SideTab::Groups {
                    return self.update_live(super::group_live::Lm::Reload);
                }
            }
            Cm::Marks(v) => {
                self.chat.marks = v.into_iter().map(|m| ((m.group, m.conversation_id), (m.message_id, m.mark))).collect();
            }
            Cm::Chats(r) => {
                // An erroring API is the empty state, not a banner.
                self.chat.chats = r.unwrap_or_default();
                self.chat.sort_chats();
                // Kept chats of people who are no longer contacts come only from this list.
                self.refresh_blocks();
            }
            Cm::Day(peer, prepend, r) => {
                if self.chat.peer.as_deref() != Some(peer.as_str()) {
                    return Task::none();
                }
                self.chat.loading = false;
                self.chat.fetching = false;
                let page = r.unwrap_or(DayPage { day: String::new(), messages: Vec::new(), older_day: None });
                let receipts = self.load_day_receipts(peer.clone(), page.day.clone());
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
                self.refresh_undelivered();
                return receipts;
            }
            Cm::Receipts(peer, r) => {
                if self.chat.peer.as_deref() == Some(peer.as_str())
                    && let Ok(list) = r
                {
                    for st in list {
                        self.chat.receipts.insert(st.message_id.clone(), st);
                    }
                    // Test-hooks: `P2P_MENU_FIRST=1` opens my oldest loaded message's menu (status + provenance screenshots).
                    #[cfg(feature = "test-hooks")]
                    if crate::test_env("P2P_MENU_FIRST").is_some() && self.chat.menu.is_none() {
                        self.chat.menu = self.chat.msgs.iter().find(|m| m.outgoing && self.chat.receipts.contains_key(&m.id)).map(|m| m.id.clone());
                    }
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
                if editing.is_none() { self.chat.compose_memory.clear_sent(&peer, &text); }
                let submission = compose_state::Submission { epoch: self.chat.account_epoch, text, reply, editing };
                let (src, p, request) = (self.src(), peer.clone(), submission.clone());
                return blocking(move || match request.editing {
                    Some(id) => src.edit_message(p, id, request.text).map_err(s),
                    None => src.send_text(p, request.text, request.reply).map_err(s),
                }, move |r| Msg::Chat(Cm::Submitted(peer.clone(), submission.clone(), r)));
            }
            Cm::Submitted(peer, submission, result) => {
                if !submission.is_current(self.chat.account_epoch) || self.lock != p2pcore::LockState::Unlocked {
                    return Task::none();
                }
                if submission.pluck(result.is_ok()) {
                    self.mark_pluck += 1;
                    // audio.ts `sent`: one faint high pluck, with the mark's pluck.
                    self.play_sound(crate::cues::cue("sent"));
                }
                let current = self.chat.peer.as_deref() == Some(peer.as_str());
                match result {
                    Ok(message) => {
                        let mut receipt = Task::none();
                        if current {
                            if submission.editing.is_some() && self.chat.editing.is_none() {
                                let draft = self.chat.compose_memory.finish_edit();
                                if self.chat.input.text().is_empty() { self.chat.input = text_editor::Content::with_text(&draft); }
                            }
                            receipt = self.load_receipt(&peer, &message.id);
                            self.chat.upsert(message);
                        }
                        return Task::batch([receipt, self.refresh_chats()]);
                    }
                    Err(error) => {
                        // Never overwrite something typed while IO was in flight.
                        if current && self.chat.input.text().is_empty() && self.chat.editing.is_none() && self.chat.reply.is_none() {
                            self.chat.input = text_editor::Content::with_text(&submission.text);
                            self.chat.reply = submission.reply;
                            self.chat.editing = submission.editing;
                        } else if !current && submission.editing.is_none() && self.chat.compose_memory.restore(&peer).text.is_empty() {
                            self.chat.compose_memory.suspend(&peer, submission.text, submission.reply, false);
                        }
                        self.notice = Some(format!("Could not send: {error}"));
                    }
                }
            }
            Cm::Done(r) => match r {
                Ok(m) => {
                    let receipt = self.load_receipt(&m.peer_did.clone(), &m.id.clone());
                    self.chat.upsert(m);
                    return receipt;
                }
                Err(e) => self.notice = Some(format!("Could not send: {e}")),
            },
            Cm::Hover(h) => self.chat.hover = h,
            Cm::Menu(id) => self.chat.menu = if self.chat.menu.as_ref() == Some(&id) { None } else { Some(id) },
            Cm::Reply(id) => {
                if self.chat.editing.is_some() {
                    self.chat.input = text_editor::Content::with_text(&self.chat.compose_memory.finish_edit());
                }
                self.chat.reply = Some(id);
                self.chat.editing = None;
                self.chat.menu = None;
                return iced::widget::operation::focus(COMPOSER_ID);
            }
            Cm::StartEdit(id) => {
                if let Some(m) = self.chat.find(&id).filter(|m| m.outgoing && !m.deleted) {
                    let text = m.text.clone();
                    self.chat.compose_memory.start_edit(self.chat.input.text());
                    self.chat.input = text_editor::Content::with_text(&text);
                    self.chat.editing = Some(id);
                    self.chat.reply = None;
                    self.chat.menu = None;
                    return iced::widget::operation::focus(COMPOSER_ID);
                }
            }
            Cm::CancelCompose => {
                if self.chat.editing.is_some() {
                    self.chat.input = text_editor::Content::with_text(&self.chat.compose_memory.finish_edit());
                }
                self.chat.editing = None;
                self.chat.reply = None;
            }
            Cm::Delete(id) => {
                self.chat.menu = None;
                self.chat.confirm_delete = Some(id);
            }
            Cm::DeleteCancel => self.chat.confirm_delete = None,
            Cm::Jump(id) => {
                let n = self.chat.msgs.len().max(2);
                let Some(i) = self.chat.msgs.iter().position(|m| m.id == id) else { return Task::none() };
                self.chat.flash = Some((id, std::time::Instant::now()));
                return iced::widget::operation::snap_to(LIST_ID, iced::widget::scrollable::RelativeOffset { x: 0.0, y: i as f32 / (n - 1) as f32 });
            }
            Cm::DeleteGo(id) => {
                self.chat.confirm_delete = None;
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
            Cm::Attach => {
                if !self.chat_visible() {
                    return Task::none();
                }
                return Task::perform(
                    async { rfd::AsyncFileDialog::new().set_title("Send a file").pick_file().await.map(|h| h.path().to_path_buf()) },
                    |p| Msg::Chat(Cm::Picked(p)),
                );
            }
            Cm::Picked(p) => {
                if let Some(p) = p {
                    return self.update_chat(Cm::Dropped(p));
                }
            }
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
            Cm::Cancel(id) => {
                let Some(peer) = self.chat.peer.clone() else { return Task::none() };
                if let Some(a) = self.chat.msgs.iter_mut().find(|m| m.id == id).and_then(|m| m.attachment.as_mut()) {
                    a.state = TransferState::Remote;
                    a.transferred = 0;
                    self.chat.progress.remove(&a.hash);
                }
                let src = self.src();
                return blocking(
                    move || src.cancel_download(peer, id).map_err(s),
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
            Cm::Find(q) => {
                let opening = self.chat.find.is_none() && q.is_some();
                self.chat.find_cur = usize::MAX;
                self.chat.find = q;
                if opening {
                    return iced::widget::operation::focus(FIND_ID);
                }
                return self.find_scroll();
            }
            Cm::FindStep(older) => {
                let n = self.find_matches().len();
                if n > 0 {
                    let i = self.chat.find_cur.min(n - 1);
                    self.chat.find_cur = if older { (i + n - 1) % n } else { (i + 1) % n };
                }
                return self.find_scroll();
            }
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
            Cm::Thumb(hash, r) => {
                let t = match r {
                    Ok(px) => Thumb::Ready { handle: iced::widget::image::Handle::from_rgba(px.w, px.h, px.rgba), w: px.w, h: px.h },
                    Err(_) => Thumb::Bad,
                };
                self.chat.thumbs.insert(hash, t);
            }
            Cm::View(id) => {
                let Some((src, peer, a)) = self.attachment_job(&id) else { return Task::none() };
                let Class::Image(ext) = media::classify(&a.name, &a.mime) else { return Task::none() };
                let body = match self.chat.thumbs.get(&a.hash) {
                    Some(Thumb::Ready { handle, .. }) => ViewBody::Image { handle: handle.clone() },
                    _ => ViewBody::Loading,
                };
                self.chat.viewer = Some(Viewer { id: id.clone(), name: a.name.clone(), body });
                self.chat.menu = None;
                return blocking(
                    move || {
                        let p = cache_attachment(&src, peer, id.clone(), &a, ext)?;
                        media::decode_scaled(&p, media::VIEW_SIDE, media::VIEW_SIDE).map(|px| (id, px))
                    },
                    |r| match r {
                        Ok((id, px)) => Msg::Chat(Cm::Viewed(id, Ok(px))),
                        Err(e) => Msg::Chat(Cm::Viewed(String::new(), Err(e))),
                    },
                );
            }
            Cm::Viewed(id, r) => {
                if let Some(v) = self.chat.viewer.as_mut() {
                    match r {
                        Ok(px) if v.id == id => {
                            v.body = ViewBody::Image { handle: iced::widget::image::Handle::from_rgba(px.w, px.h, px.rgba) }
                        }
                        Err(e) if matches!(v.body, ViewBody::Loading) => v.body = ViewBody::Failed(e),
                        _ => {}
                    }
                }
            }
            Cm::Open(id) => {
                let Some((src, peer, a)) = self.attachment_job(&id) else { return Task::none() };
                match media::classify(&a.name, &a.mime) {
                    Class::Text(ext) if a.size <= media::TEXT_MAX => {
                        self.chat.viewer = Some(Viewer { id: id.clone(), name: a.name.clone(), body: ViewBody::Loading });
                        return blocking(
                            move || cache_attachment(&src, peer, id, &a, ext).and_then(|p| media::read_text(&p)),
                            |r| Msg::Chat(Cm::TextShown(r)),
                        );
                    }
                    _ => return self.update_chat(Cm::OpenExt(id)),
                }
            }
            Cm::OpenExt(id) => {
                let Some((src, peer, a)) = self.attachment_job(&id) else { return Task::none() };
                let ext = match media::classify(&a.name, &a.mime) {
                    Class::Image(e) | Class::Text(e) | Class::Open(e) => e,
                    Class::SaveOnly => return Task::none(),
                };
                return blocking(
                    move || {
                        let p = cache_attachment(&src, peer, id, &a, ext)?;
                        media::open_external(&p)
                    },
                    |r| Msg::Chat(Cm::Opened(r)),
                );
            }
            Cm::TextShown(r) => {
                if let Some(v) = self.chat.viewer.as_mut().filter(|v| matches!(v.body, ViewBody::Loading)) {
                    v.body = match r {
                        Ok(t) => ViewBody::Text(t),
                        Err(e) => ViewBody::Failed(e),
                    };
                }
            }
            Cm::Opened(r) => {
                if let Err(e) = r {
                    self.notice = Some(format!("Could not open: {e}"));
                }
            }
            Cm::CloseView => self.chat.viewer = None,
            Cm::Step(d) => {
                let Some(cur) = self.chat.viewer.as_ref().map(|v| v.id.clone()) else { return Task::none() };
                let ids = self.chat.viewable_images();
                if let Some(i) = ids.iter().position(|x| *x == cur) {
                    let n = i as i32 + d;
                    if n >= 0 && (n as usize) < ids.len() {
                        return self.update_chat(Cm::View(ids[n as usize].clone()));
                    }
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
        let t = self.on_chat_event_inner(ev);
        self.sync_ticks();
        Task::batch([t, self.ensure_thumbs(), self.refresh_marks()])
    }

    fn on_chat_event_inner(&mut self, ev: ChatEv) -> Task<Msg> {
        match ev {
            ChatEv::Added(m) => {
                let (peer, incoming) = (m.peer_did.clone(), !m.outgoing);
                let open = self.chat_visible() && self.chat.peer.as_deref() == Some(peer.as_str());
                // Seen only if the window exists and the user is in it.
                let attended = open && self.win.is_some() && self.focused;
                if incoming && !attended && crate::app::NOTIFY_MESSAGES.load(std::sync::atomic::Ordering::Relaxed) {
                    notify(&self.chat_name(&peer), &preview_of(&m), Target::Chat(peer.clone()));
                }
                // audio.ts `received`: the can, tapped once, for a message in front of me and for one that raised a notification
                // (the window away); Notifications > Messages and blocks apply, and Quiet is checked by the player.
                if incoming && crate::app::NOTIFY_MESSAGES.load(std::sync::atomic::Ordering::Relaxed) && !self.blocks.blocked_by_me(&peer) {
                    self.play_received();
                }
                let receipt = if open && !incoming { self.load_receipt(&peer, &m.id) } else { Task::none() };
                self.chat.upsert(m);
                if open && !incoming {
                    self.refresh_undelivered();
                }
                if incoming && attended {
                    return Task::batch([receipt, self.mark_read(&peer)]);
                }
                return receipt;
            }
            // Also raised when the contact's signed receipts for the shown version were committed.
            ChatEv::Changed(m) => {
                let receipt = if m.outgoing { self.load_receipt(&m.peer_did, &m.id) } else { Task::none() };
                self.chat.upsert(m);
                return receipt;
            }
            ChatEv::Presence(peer, on) => {
                if on {
                    self.chat.online.insert(peer);
                } else {
                    self.chat.online.remove(&peer);
                }
            }
            ChatEv::Chat(c) => self.chat.upsert_chat(c),
            ChatEv::Delivery(peer, id, d) => {
                if self.chat.peer.as_deref() == Some(peer.as_str())
                    && let Some(m) = self.chat.msgs.iter_mut().find(|m| m.id == id)
                {
                    m.delivery = d;
                    self.refresh_undelivered();
                    // Only the "still only here" part of the mark can change with this.
                    return self.load_receipt(&peer, &id);
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
