//! The phone's (and desktop's) whole P2P side behind one UniFFI object: identity, contacts,
//! the iroh endpoint, the call protocol and the codec. Platforms bring UI and audio devices.

#![deny(clippy::string_slice)]

uniffi::setup_scaffolding!();

#[macro_use]
mod sink_adapter;

// Links `present` (and its UniFFI scaffolding) into libp2pcore.so: bindgen then writes uniffi/present/present.kt.
pub use present;

// The account model (state, account dirs, vault, chat day doc, own-device docs) is the
// `account-model` crate; its modules keep their old paths here.
pub use account_model::accounts;
pub mod avatar;
mod block;
mod caps;
mod chat;
mod contact_req;
mod devlist;
mod profile;
pub use profile::{AccountProfile, ProfileEvents};
mod group;
mod error;
pub mod gcall;
pub mod cues;
mod licences;
mod logging;
mod node;
mod recovery_key;
mod rekey;
mod receipts;
mod search;
mod store;
mod sync;
/// Synthetic peers, contacts and groups for tests and benches; not in release builds.
#[cfg(any(test, feature = "dev-tools"))]
pub mod synth;
#[cfg(test)]
mod account_tests;
use account_model::vault;
mod video_call;
#[cfg(target_os = "android")]
mod video_jni;
mod voice;

pub use error::Error;

pub use block::{BLOCKED, BlockNotice, BlockState, BlockedContact};
pub use group::api::{GroupDelivery, GroupDetail, GroupEvents, GroupFlag, GroupHost, GroupImage, GroupImageState, GroupInvitePeek, GroupItemKind, GroupMember, GroupMessage, GroupPage, GroupRemoval, GroupRole, GroupSummary, OnlineMember};
pub use group::requests::{ChatRequestState, GroupChatRequest, GroupRequestEvents};
pub use group::rotation::RotationPolicy;
pub use group::calls::{GroupCall, GroupCallConditions, GroupCallDeviceClass, GroupCallEvent, GroupCallEventKind, GroupCallEvents, GroupCallHere, GroupCallThermal};
pub use group::calllog::{GroupCallRecord, group_call_label};
pub use group::invitations::{GroupInvitation, GroupInvitationEvents, GroupInvitationState};
pub use caps::{CALLS_NOT_ALLOWED, ContactOrigin};
pub use contact_req::{AddError, AddOutcome, CodePeek, ContactRequest, ContactRequestEvents, MyCode};
pub use search::api::{HitRange, MessageHit, NameHit, SearchConvKind, SearchNameKind, SearchResults};
pub use receipts::api::{ChatReceiptRow, ReceiptTick, MessageReceiptStatus, ReceiptLevel, ReceiptProof, ReceiptProofKind, RecipientReceipts};
pub use group::member_receipts::{GroupMemberReceipts, GroupReceiptStatus};
pub use receipts::list::ListMark;
pub use receipts::ticks::{TickCurve, TickPose, TickTransition, TickVis, tick_pose, tick_transition, tick_vis};
pub use chat::api::{Attachment, AttachmentKind, Chat, ChatEvents, DayPage, DeliveryState, Message, PictureDownload, StorageUsage, TransferState};
pub use sync::{LinkEvents, LinkedDevice};
pub use voice::{VoiceDecoder, VoiceInfo, VoiceRecorder};
pub use recovery_key::{is_recovery_key, recovery_key_from_phrase, recovery_phrase_from_input};
pub use store::CallRecord;
pub use video_call::{video_codec_label, video_recv_label, video_send_label};
pub use video_call::{Bytes, Pulled, VideoBudget, VideoEvents, VideoFrame, VideoInfo, VideoSendCap, VideoStats, VideoTuning, video_tuning_stats_label};
pub use video_call::{VideoChoice, VideoLive, VideoStatRow, video_stat_rows};
pub use node::{AccountSummary, Availability, CallInfo, CallState, CallStats, CardPeek, Contact, LockState, Node, NodeEvents, NodeStatus, ProfileInfo};
pub use licences::{open_source_licences, OpenSourceLicence};

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
/// Only with the `dev-tools` feature.
#[cfg(feature = "dev-tools")]
#[uniffi::export]
pub fn video_bench(y4m_path: Option<String>, frames: u32, threads: u32, pin_fast_cores: bool) -> Result<String, Error> {
    video::bench::run(y4m_path.as_deref(), frames as usize, threads as usize, 10, pin_fast_cores).map_err(Error::Protocol)
}
