//! The records of the chat API and the plain event sink the engine reports to. p2pcore declares
//! the records to UniFFI (`#[uniffi::remote]`) and adapts its foreign `ChatEvents` to
//! [`ChatSink`]; see `docs/chat.md` "API".

pub use super::store::PictureDownload;

/// Bytes held on this device for chats (Settings > Storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    Delivered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    File,
    Voice,
}

/// Where the attachment's bytes are on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    /// Not on this device yet (too big for auto-download, or the peer was offline).
    Remote,
    Downloading,
    /// Held here; `save_attachment` decrypts it to a path.
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
pub struct DayPage {
    pub day: String,
    pub messages: Vec<Message>,
    /// The next older day that has messages on this device, to ask for next; `None` at the
    /// start of what we hold (`fetch_older_history` can ask the peer for more).
    pub older_day: Option<String>,
}

/// What the chat engine reports (p2pcore forwards it to the app's `ChatEvents`). Called on core
/// threads: do not block.
pub trait ChatSink: Send + Sync {
    /// A message appeared (sent here, or arrived).
    fn on_message_added(&self, message: Message);
    /// Edited, deleted, attachment state changed.
    fn on_message_changed(&self, message: Message);
    /// The conversation list row changed (new message, unread, preview).
    fn on_chat_changed(&self, chat: Chat);
    /// A message we sent changed delivery state.
    fn on_delivery_changed(&self, peer_did: String, message_id: String, delivery: DeliveryState);
    /// Bytes moved for an attachment; `outgoing` = the peer is fetching ours. `done == 0 &&
    /// total == 0` means the transfer ended without completing.
    fn on_transfer_progress(&self, peer_did: String, hash: String, done: u64, total: u64, outgoing: bool);
    /// The contact became reachable or stopped being so.
    fn on_presence_changed(&self, peer_did: String, online: bool);
}
