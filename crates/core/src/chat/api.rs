//! The UniFFI surface of chat (frozen shapes; see `docs/chat.md` "API"). Everything is additive
//! to `Node`/`NodeEvents`. Calls on a `Node` that is locked fail with `Locked`.
//!
//! Conventions: `peer_did` names the conversation (1:1 only for now); times are unix
//! milliseconds; a "day" is the UTC date `YYYY-MM-DD` (the sync shard). Blocking methods are for
//! the app's IO threads, like the rest of `Node`.

use std::sync::Arc;

use crate::node::Node;
use std::path::PathBuf;
use crate::Error;

pub use super::store::PictureDownload;

// The records are `chat_runtime::chat::api`; UniFFI declarations here (the declaring module is
// part of the type metadata, so they stay in this module).
pub use chat_runtime::chat::api::{Attachment, AttachmentKind, Chat, DayPage, DeliveryState, Message, StorageUsage, TransferState};

/// Bytes held on this device for chats (Settings > Storage).
#[uniffi::remote(Record)]
pub struct StorageUsage {
    /// Complete attachments whose type is an image.
    pub pictures: u64,
    /// Every other complete attachment, voice messages included.
    pub files: u64,
    /// The chat file: messages, reactions, sync state.
    pub messages: u64,
}

/// Honest ticks. For a message we sent: `Pending` = only on this phone (one tick), `Delivered`
/// = the peer's device confirmed it holds it (two ticks). Messages we received are `Delivered`.
#[uniffi::remote(Enum)]
pub enum DeliveryState {
    Pending,
    Delivered,
}

#[uniffi::remote(Enum)]
pub enum AttachmentKind {
    File,
    Voice,
}

/// Where the attachment's bytes are on this device.
#[uniffi::remote(Enum)]
pub enum TransferState {
    /// Not on this device yet (too big for auto-download, or the peer was offline).
    Remote,
    Downloading,
    /// Held here; `save_attachment` decrypts it to a path.
    Ready,
    Failed,
}

#[uniffi::remote(Record)]
pub struct Attachment {
    /// BLAKE3 of the ciphertext, hex; identifies the transfer in `on_transfer_progress`.
    pub hash: String,
    pub name: String,
    /// Plaintext size in bytes.
    pub size: u64,
    pub mime: String,
    pub kind: AttachmentKind,
    /// Voice only, else 0.
    pub duration_ms: u32,
    /// Voice only: about 64 peaks, 0..=255; empty otherwise.
    pub waveform: Vec<u8>,
    /// Pictures sent by /5 apps: the size it is shown at (orientation applied), else 0x0.
    pub width: u32,
    pub height: u32,
    /// Pictures: a 4x3 grid of its colours, RGB row by row (36 bytes), to draw scaled up and soft until the picture
    /// is here; empty when unknown.
    pub preview: Vec<u8>,
    pub state: TransferState,
    /// Bytes transferred so far while `Downloading`, else 0.
    pub transferred: u64,
}

#[uniffi::remote(Record)]
pub struct Message {
    pub id: String,
    pub peer_did: String,
    pub author_did: String,
    pub outgoing: bool,
    /// When the author sent it (unix ms); the order key, then `id`.
    pub at: u64,
    /// Empty when `deleted`.
    pub text: String,
    pub edited_at: Option<u64>,
    pub deleted: bool,
    pub reply_to: Option<String>,
    pub attachment: Option<Attachment>,
    pub delivery: DeliveryState,
}

/// One row of the conversation list.
#[uniffi::remote(Record)]
pub struct Chat {
    pub peer_did: String,
    pub peer_name: String,
    /// Text of the last message, or "Voice message" / the file name; empty if none.
    pub preview: String,
    pub last_outgoing: bool,
    pub last_delivery: DeliveryState,
    /// Unix ms of the last message, or 0.
    pub last_activity: u64,
    pub unread: u32,
    /// The id of the last message (what the list's mark is for, `chat_list_marks`); `None` if none.
    pub last_id: Option<String>,
}

/// One UTC day of a conversation, oldest message first.
#[uniffi::remote(Record)]
pub struct DayPage {
    pub day: String,
    pub messages: Vec<Message>,
    /// The next older day that has messages on this device, to ask for next; `None` at the
    /// start of what we hold (`fetch_older_history` can ask the peer for more).
    pub older_day: Option<String>,
}

/// Registered with `Node::set_chat_events`; separate from `NodeEvents`. Called on core threads:
/// do not block, and do not call blocking `Node` methods from inside.
#[uniffi::export(rust, foreign)]
pub trait ChatEvents: Send + Sync {
    /// A message appeared (sent here, or arrived).
    fn on_message_added(&self, message: Message);
    /// Edited, deleted, attachment state changed.
    fn on_message_changed(&self, message: Message);
    /// The conversation list row changed (new message, unread, preview).
    fn on_chat_changed(&self, chat: Chat);
    /// A message we sent changed delivery state.
    fn on_delivery_changed(&self, peer_did: String, message_id: String, delivery: DeliveryState);
    /// Bytes moved for an attachment; `outgoing` = the peer is fetching ours. `done == 0 &&
    /// total == 0` means the transfer ended without completing (outgoing: the peer dropped or
    /// the link died, so leave "Sending"; incoming: cancelled).
    fn on_transfer_progress(&self, peer_did: String, hash: String, done: u64, total: u64, outgoing: bool);
    /// The contact became reachable (one of their devices is live on a chat link) or stopped
    /// being so. Ephemeral: nothing is stored; at start every contact is offline.
    fn on_presence_changed(&self, peer_did: String, online: bool);
}

sink_adapter!(
    /// Hands the chat engine's events to the app's `ChatEvents`.
    pub(crate) ChatEventsAdapter(ChatEvents) => chat_runtime::ChatSink {
        fn on_message_added(message: Message);
        fn on_message_changed(message: Message);
        fn on_chat_changed(chat: Chat);
        fn on_delivery_changed(peer_did: String, message_id: String, delivery: DeliveryState);
        fn on_transfer_progress(peer_did: String, hash: String, done: u64, total: u64, outgoing: bool);
        fn on_presence_changed(peer_did: String, online: bool);
    }
);

#[uniffi::export]
impl Node {
    pub fn set_chat_events(&self, events: Arc<dyn ChatEvents>) {
        self.hub.set_sinks(|s| s.chat = Some(Arc::new(ChatEventsAdapter(events))));
    }

    /// Conversations, most recent activity first. One per contact.
    pub fn chats(&self) -> Result<Vec<Chat>, Error> {
        Ok(self.open().chat.chat_list()?)
    }

    /// The messages of one UTC day; `day = None` is the newest day that has any. Only today's
    /// shard is loaded eagerly; older days are read from the local store when asked.
    pub fn chat_day(&self, peer_did: String, day: Option<String>) -> Result<DayPage, Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_day(&peer_did, day).await })
    }

    /// Asks the peer for days older than `before_day` that we do not hold (needs a connection;
    /// blocks until answered or timed out). Returns how many days were added.
    pub fn fetch_older_history(&self, peer_did: String, before_day: String) -> Result<u32, Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_fetch_history(&peer_did, before_day).await })
    }

    pub fn send_text(&self, peer_did: String, text: String, reply_to: Option<String>) -> Result<Message, Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_send_message(&peer_did, text, reply_to, None).await })
    }

    /// Our own message only.
    pub fn edit_message(&self, peer_did: String, message_id: String, text: String) -> Result<Message, Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_edit(&peer_did, &message_id, Some(text)).await })
    }

    /// Our own message only; clears the text, keeps the id.
    pub fn delete_message(&self, peer_did: String, message_id: String) -> Result<Message, Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_edit(&peer_did, &message_id, None).await })
    }

    /// Whether a session with the contact is up and it reports itself online (see
    /// `ChatEvents::on_presence_changed`). A contact sends presence only while it is active
    /// with us (consent-based links, 2026-10-17), so false means "unknown", not "away".
    pub fn contact_online(&self, peer_did: String) -> bool {
        self.open().chat.presence_of(&peer_did)
    }

    /// The user is typing in the conversation with `peer_did` (call it on each non-empty
    /// composer edit; cheap, non-blocking, throttled inside). Dials the contact ahead of the
    /// send and counts as activity with it: the session stays open and carries our presence
    /// for the idle window. Nothing about the typing is sent to the contact.
    pub fn chat_typing(&self, peer_did: String) {
        self.open().chat.chat_typing(&peer_did);
    }

    /// Zeroes the local unread counter. Also signs a recipient read receipt for exactly the
    /// incoming versions this step marks read (`chat_message_receipts`); sending it to the
    /// contact is not wired yet (receipt exchange, roadmap M4).
    /// Blocking (an old unread day may be read from its blob first).
    pub fn mark_read(&self, peer_did: String) -> Result<(), Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_mark_read(&peer_did).await })
    }

    /// Encrypts the file at `path` (up to 2 GB) and sends a message carrying it. Blocks while
    /// encrypting; progress is not reported for that step.
    pub fn send_file(
        &self,
        peer_did: String,
        path: String,
        mime: String,
        text: Option<String>,
    ) -> Result<Message, Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_send_file(&peer_did, PathBuf::from(path), mime, text, None).await })
    }

    /// Sends an already recorded voice file (Ogg Opus, 16 kHz mono; produced by the app's
    /// recorder) as a blob with `kind = Voice`. `waveform` is about 64 peaks, 0..=255.
    pub fn send_voice(
        &self,
        peer_did: String,
        path: String,
        duration_ms: u32,
        waveform: Vec<u8>,
    ) -> Result<Message, Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move {
            chat.chat_send_file(&peer_did, PathBuf::from(path), "audio/ogg".into(), None, Some((duration_ms, waveform))).await
        })
    }

    /// Starts fetching the attachment of a message now (progress via `on_transfer_progress`,
    /// the end via `on_message_changed`). Returns at once.
    pub fn download_attachment(&self, peer_did: String, message_id: String) -> Result<(), Error> {
        Ok(self.open().chat.chat_want(&peer_did, &message_id)?)
    }

    /// Gives up a download in progress (or one waiting to retry, or failed): the transfer stops,
    /// the partial bytes are dropped (freed within ~30 s) and the attachment is `Remote` again
    /// ("Tap to download"); `download_attachment` later starts from the beginning. A `Ready`
    /// attachment is left alone. The end is reported via `on_message_changed`.
    pub fn cancel_download(&self, peer_did: String, message_id: String) -> Result<(), Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_cancel(&peer_did, &message_id).await })
    }

    /// Decrypts a `Ready` attachment to `dest_path` (a voice message is saved the same way and
    /// played from the file). Blocks.
    pub fn save_attachment(&self, peer_did: String, message_id: String, dest_path: String) -> Result<(), Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_save(&peer_did, &message_id, PathBuf::from(dest_path)).await })
    }

    /// Attachments up to this many bytes download by themselves; 0 = never. Default 10 MB.
    pub fn set_auto_download_limit(&self, bytes: u64) -> Result<(), Error> {
        self.open().chat.chat_core()?.store.set_auto_download(bytes).map_err(Error::from)
    }

    pub fn auto_download_limit(&self) -> Result<u64, Error> {
        Ok(self.open().chat.chat_core()?.store.auto_download())
    }

    /// Settings > Storage "Download pictures" (default `Always`). The size limit above still applies.
    pub fn picture_download(&self) -> Result<PictureDownload, Error> {
        Ok(self.open().chat.chat_core()?.store.picture_download())
    }

    pub fn set_picture_download(&self, policy: PictureDownload) -> Result<(), Error> {
        self.open().chat.chat_core()?.store.set_picture_download(policy).map_err(Error::from)
    }

    /// Settings > Privacy "Read receipts" (default on, per account on this device). Off: this
    /// account sends no read receipts in chats and groups; delivered receipts still go, and
    /// other people's reads still show. Reads made while off are never confirmed later.
    pub fn read_receipts(&self) -> Result<bool, Error> {
        Ok(self.open().chat.chat_core()?.store.read_receipts())
    }

    pub fn set_read_receipts(&self, on: bool) -> Result<(), Error> {
        self.open().chat.chat_core()?.store.set_read_receipts(on).map_err(Error::from)
    }

    /// Tells the core whether the connection is metered (mobile data), for `PictureDownload::Wifi`.
    /// Process-wide; the app calls it when the network changes.
    pub fn set_connection_metered(&self, metered: bool) {
        crate::chat::store::set_metered(metered);
    }

    /// What Tinline keeps for chats on this device (Settings > Storage): complete attachments by kind, and the
    /// chat file itself. Blocks (reads the chat records and one file size).
    pub fn storage_usage(&self) -> Result<StorageUsage, Error> {
        let core = self.open().chat.chat_core()?;
        let (pictures, files) = core.store.blob_usage()?;
        let messages = std::fs::metadata(core.dir.join("chat.redb")).map(|m| m.len()).unwrap_or(0);
        Ok(StorageUsage { pictures, files, messages })
    }
}

impl Node {
    /// Not exported to apps: tests use it to show that a contact who is not a party to a
    /// conversation cannot fetch its blobs.
    #[doc(hidden)]
    pub fn raw_fetch_blob(&self, did: String, hash: String, size: u64) -> Result<u64, Error> {
        let chat = self.open().chat.clone();
        self.block_rt(async move { chat.chat_raw_fetch(&did, &hash, size).await })
    }
}
