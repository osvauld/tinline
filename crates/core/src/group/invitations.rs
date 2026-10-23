//! Inviting contacts directly (G3). The runtime is `group_runtime::invitations`; this declares
//! its records to UniFFI, forwards its events to the app and holds the `Node` calls (`node.rs`).


pub use group_runtime::invitations::*;

// The state lives with the stored rows (`group_core::invitations`); this declares it to UniFFI.
#[uniffi::remote(Enum)]
pub enum GroupInvitationState {
    /// Not answered yet.
    Pending,
    /// The invitee joined the group.
    Joined,
    Declined,
    /// The invite ran out (7 days) before it was answered.
    Expired,
    /// The invitee blocks the inviter (P5): it was never shown to them.
    Blocked,
}

#[uniffi::remote(Record)]
pub struct GroupInvitation {
    pub id: String,
    pub group_id: String,
    pub group_name: String,
    pub inviter_did: String,
    pub inviter_name: String,
    pub invitee_did: String,
    pub invitee_name: String,
    /// We invited (`true`) or were invited.
    pub outgoing: bool,
    pub state: GroupInvitationState,
    /// Unix ms.
    pub at: u64,
    /// Unix ms.
    pub expires: u64,
    /// Outgoing: one of the invitee's devices took it (or someone answered).
    pub delivered: bool,
}

/// Registered with `Node::set_group_invitation_events`. Called on core threads: do not block.
#[uniffi::export(rust, foreign)]
pub trait GroupInvitationEvents: Send + Sync {
    /// An invitation appeared or changed state (either direction).
    fn on_group_invitation(&self, invitation: GroupInvitation);
}

sink_adapter!(
    /// Hands the runtime's events to the app's `GroupInvitationEvents`.
    pub(crate) GroupInvitationEventsAdapter(GroupInvitationEvents) => group_runtime::invitations::GroupInvitationSink {
        fn on_group_invitation(invitation: GroupInvitation);
    }
);

mod node;
