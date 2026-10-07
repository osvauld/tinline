//! The phone's (and desktop's) whole P2P side behind one UniFFI object: identity, contacts,
//! the iroh endpoint, the call protocol and the codec. Platforms bring UI and audio devices.

uniffi::setup_scaffolding!();

mod chat;
mod error;
mod logging;
mod node;
mod store;
mod vault;
mod wire;

pub use error::Error;
pub use store::CallRecord;
pub use node::{Availability, CallInfo, CallState, CallStats, Contact, LockState, Node, NodeEvents, NodeStatus, ProfileInfo};

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
