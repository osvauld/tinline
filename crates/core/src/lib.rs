//! The phone's (and desktop's) whole P2P side behind one UniFFI object: identity, contacts,
//! the iroh endpoint, the call protocol and the codec. Platforms bring UI and audio devices.

uniffi::setup_scaffolding!();

pub mod accounts;
mod chat;
mod error;
mod logging;
mod node;
mod store;
#[cfg(test)]
mod account_tests;
mod vault;
mod voice;
mod wire;

pub use error::Error;
pub use chat::api::{Attachment, AttachmentKind, Chat, ChatEvents, DayPage, DeliveryState, Message, TransferState};
pub use voice::{VoiceDecoder, VoiceInfo, VoiceRecorder};
pub use store::CallRecord;
pub use node::{AccountSummary, Availability, CallInfo, CallState, CallStats, CardPeek, Contact, LockState, Node, NodeEvents, NodeStatus, ProfileInfo};

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
