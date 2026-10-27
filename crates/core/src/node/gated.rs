//! The platform's sinks as the open slot holds them (`slots.rs`): each event passes only while
//! that slot is still the open one, and open since the sink was installed. A task of the account
//! just left may still hold a clone of the sink from before the switch (a batch being applied, a
//! transfer in flight); what it reports then goes nowhere instead of onto the new account's
//! screens.

use std::sync::Arc;

use super::SlotGate;
use super::{Contact, ContactRequest, ContactRequestSink};
use chat_runtime::chat::api::{Chat, DeliveryState, Message};

/// Defines `$name`, a `$tr` that forwards to the platform's while the slot's gate passes.
macro_rules! gated {
    ($name:ident, $tr:path { $(fn $m:ident(&self $(, $a:ident: $t:ty)*);)* }) => {
        pub(crate) struct $name {
            inner: Arc<dyn $tr>,
            gate: Arc<SlotGate>,
            generation: u64,
        }

        impl $name {
            pub(crate) fn new(inner: Arc<dyn $tr>, gate: &Arc<SlotGate>) -> Arc<dyn $tr> {
                Arc::new(Self { inner, gate: gate.clone(), generation: gate.generation() })
            }
        }

        impl $tr for $name {
            $(fn $m(&self $(, $a: $t)*) {
                if self.gate.passes(self.generation) {
                    self.inner.$m($($a),*);
                }
            })*
        }
    };
}

gated!(GatedChat, chat_runtime::ChatSink {
    fn on_message_added(&self, message: Message);
    fn on_message_changed(&self, message: Message);
    fn on_chat_changed(&self, chat: Chat);
    fn on_delivery_changed(&self, peer_did: String, message_id: String, delivery: DeliveryState);
    fn on_transfer_progress(&self, peer_did: String, hash: String, done: u64, total: u64, outgoing: bool);
    fn on_presence_changed(&self, peer_did: String, online: bool);
});

gated!(GatedProfile, chat_runtime::ProfileSink {
    fn on_profile_changed(&self, did: String);
});

gated!(GatedLink, sync_runtime::LinkSink {
    fn on_link_code(&self, code: String, peer_label: String);
    fn on_link_done(&self, did: String, name: String);
    fn on_link_failed(&self, reason: String);
    fn on_devices_changed(&self);
    fn on_history_changed(&self);
    fn on_unlinked(&self, did: String);
});

gated!(GatedGroup, group_runtime::api::GroupSink {
    fn on_group_message(&self, message: group_runtime::api::GroupMessage);
    fn on_group_changed(&self, group_id: String);
    fn on_group_presence(&self, group_id: String);
    fn on_group_image(&self, group_id: String, message_id: String);
});

gated!(GatedGroupCall, group_runtime::calls::GroupCallSink {
    fn on_group_call(&self, event: group_runtime::calls::GroupCallEvent);
});

gated!(GatedInvitation, group_runtime::invitations::GroupInvitationSink {
    fn on_group_invitation(&self, invitation: group_runtime::invitations::GroupInvitation);
});

gated!(GatedRequest, group_runtime::requests::GroupRequestSink {
    fn on_chat_request(&self, request: group_runtime::requests::GroupChatRequest);
});

gated!(GatedContactRequest, ContactRequestSink {
    fn on_contact_request(&self, request: ContactRequest);
    fn on_request_accepted(&self, contact: Contact);
});

#[cfg(test)]
mod tests;
