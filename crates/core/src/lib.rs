//! The phone's (and desktop's) whole P2P side behind one UniFFI object: identity, contacts,
//! the iroh endpoint, the call protocol and the codec. Platforms bring UI and audio devices.

uniffi::setup_scaffolding!();

pub mod accounts;
mod block;
mod caps;
mod chat;
mod devlist;
mod group;
mod error;
pub mod gcall;
mod logging;
mod node;
mod rekey;
mod store;
mod sync;
mod version;
#[cfg(test)]
mod account_tests;
mod vault;
mod video_call;
#[cfg(target_os = "android")]
mod video_jni;
mod voice;
mod wire;

pub use error::Error;
pub use block::{BLOCKED, BlockNotice, BlockState, BlockedContact};
pub use group::api::{GroupDelivery, GroupDetail, GroupEvents, GroupFlag, GroupHost, GroupInvitePeek, GroupItemKind, GroupMember, GroupMessage, GroupPage, GroupRemoval, GroupRole, GroupSummary};
pub use group::requests::{ChatRequestState, GroupChatRequest, GroupRequestEvents};
pub use group::calls::{GroupCall, GroupCallEvent, GroupCallEventKind, GroupCallEvents};
pub use group::invitations::{GroupInvitation, GroupInvitationEvents, GroupInvitationState};
pub use caps::{CALLS_NOT_ALLOWED, ContactOrigin};
pub use chat::api::{Attachment, AttachmentKind, Chat, ChatEvents, DayPage, DeliveryState, Message, TransferState};
pub use sync::{LinkEvents, LinkedDevice};
pub use voice::{VoiceDecoder, VoiceInfo, VoiceRecorder};
pub use store::CallRecord;
pub use video_call::{Bytes, Pulled, VideoEvents, VideoFrame, VideoInfo, VideoStats};
pub use node::{AccountSummary, Availability, CallInfo, CallState, CallStats, CardPeek, Contact, LockState, Node, NodeEvents, NodeStatus, ProfileInfo};

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The DID a recovery phrase belongs to (`BadPhrase` if it is not valid), so a platform can offer
/// "Open <that account>" when the phrase is already on this device.
#[uniffi::export]
pub fn did_of_phrase(phrase: String) -> Result<String, Error> {
    let phrase = zeroize::Zeroizing::new(phrase);
    identity::recover(phrase.trim()).map(|i| i.did().to_string()).map_err(|_| Error::BadPhrase)
}

/// Video codec benchmark (libaom realtime encode + rav1d decode) run inside this process, so the
/// numbers reflect how the OS schedules the app's threads. `y4m_path` = real footage, else a
/// synthetic pattern. Debug builds trigger it via DebugReceiver `video_bench`.
#[uniffi::export]
pub fn video_bench(y4m_path: Option<String>, frames: u32, threads: u32, pin_fast_cores: bool) -> Result<String, Error> {
    video::bench::run(y4m_path.as_deref(), frames as usize, threads as usize, 10, pin_fast_cores).map_err(Error::Protocol)
}
