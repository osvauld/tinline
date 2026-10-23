//! The plain mirrors of the app-facing callback traits. p2pcore wraps the UniFFI callback objects
//! (`NodeEvents`, `ContactRequestEvents`) in adapters that implement these.

use crate::{CallInfo, CallState, Contact, ContactRequest, NodeStatus};

/// Everything the node tells the app about itself and its 1:1 calls. Called on core threads:
/// implementations must not block.
pub trait NodeSink: Send + Sync {
    fn on_status(&self, status: NodeStatus);
    fn on_contacts_changed(&self);
    fn on_incoming_call(&self, call: CallInfo);
    fn on_call_state(&self, call_id: String, state: CallState);
    fn on_log(&self, line: String);
}

/// Contact requests (`contact_req`). Called on core threads: implementations must not block.
pub trait ContactRequestSink: Send + Sync {
    /// Someone used our code: "Maya wants to add you as a contact". Once per joiner (a repeated
    /// hello is the same request).
    fn on_contact_request(&self, request: ContactRequest);
    /// They accepted the request we sent: our line to them is open.
    fn on_request_accepted(&self, contact: Contact);
}
