//! Blocking and call permissions, from plain inputs. The core owns the facts
//! (`Node::block_state`, `calls_allowed`, `can_call`); this decides what the screens do with them.
//!
//! Personal blocking is its own policy: a block does not touch groups, delegation or devices.
//! Decisions (docs/design/rotation-open-decisions.md section 0): a block is visible to the blocked
//! person; a contact's action is Block (no "Remove contact"); chats are never deleted.

use crate::names::first_word;

/// What sits where the composer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ComposerState {
    Open,
    /// We block them: "You blocked X" and Unblock.
    BlockedByMe,
    /// They block us: no composer, just why.
    BlockedMe,
    /// A kept chat with someone who is no longer a contact: add them again before messaging.
    NotContact,
}

/// Why the call button does or doesn't dial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CallGate {
    Ok,
    /// Blocked on either side, or no longer a contact ([`composer_state`] says which).
    Closed,
    /// They let us message them but not call (no call token from them).
    NotAllowed,
}

/// The composer for a conversation. `contact`: they are in our contacts (or it is the "You"
/// conversation). `blocked_me`: they sent us a block notice.
#[uniffi::export]
pub fn composer_state(blocked_by_me: bool, blocked_me: bool, contact: bool) -> ComposerState {
    if blocked_by_me {
        ComposerState::BlockedByMe
    } else if blocked_me {
        ComposerState::BlockedMe
    } else if !contact {
        ComposerState::NotContact
    } else {
        ComposerState::Open
    }
}

/// The call button. `holds_call_token`: they let us call them.
#[uniffi::export]
pub fn call_gate(composer: ComposerState, holds_call_token: bool) -> CallGate {
    match composer {
        ComposerState::Open if holds_call_token => CallGate::Ok,
        ComposerState::Open => CallGate::NotAllowed,
        _ => CallGate::Closed,
    }
}

/// The red line under a name in lists and headers; `None` when there is no block.
#[uniffi::export]
pub fn block_status(blocked_by_me: bool, blocked_me: bool) -> Option<String> {
    match composer_state(blocked_by_me, blocked_me, true) {
        ComposerState::BlockedByMe => Some("Blocked".into()),
        ComposerState::BlockedMe => Some("Blocked you".into()),
        _ => None,
    }
}

fn f(name: &str) -> &str {
    first_word(name)
}

/// "Block X?"
#[uniffi::export]
pub fn block_dialog_title(name: String) -> String {
    format!("Block {}?", f(&name))
}

/// The consequences, listed before the tap.
#[uniffi::export]
pub fn block_dialog_lines(name: String) -> Vec<String> {
    let f = f(&name);
    vec![
        format!("{f} can\u{2019}t message or call you, on any of your devices."),
        format!(
            "{f} will see that you blocked them, and their app removes you from their contacts."
        ),
        format!("Your chat history with {f} stays."),
        format!("To talk again after you unblock, {f} has to add you again with your code."),
        "Groups you share are not affected.".into(),
    ]
}

/// Under one of our messages that will not arrive.
#[uniffi::export]
pub fn not_delivered_text(name: String) -> String {
    format!("Not delivered: {} blocked you", f(&name))
}

/// Title of the notice in the blocked person's chat.
#[uniffi::export]
pub fn blocked_you_title(name: String) -> String {
    format!("{} blocked you", f(&name))
}

/// Body of that notice.
#[uniffi::export]
pub fn blocked_you_body(name: String) -> String {
    format!(
        "You can\u{2019}t message or call {}. Your earlier messages stay here.",
        f(&name)
    )
}

/// What the blocker sees in place of the composer.
#[uniffi::export]
pub fn blocked_by_me_line(name: String) -> String {
    format!(
        "You blocked {}. They can\u{2019}t message or call you.",
        f(&name)
    )
}

/// In place of the composer of a kept chat with someone who is no longer a contact.
#[uniffi::export]
pub fn not_contact_line(name: String) -> String {
    let f = f(&name);
    format!("{f} is no longer in your contacts. Add {f} again with their code to message.")
}

/// Android's "can't send" line for a closed composer.
#[uniffi::export]
pub fn cant_send_text(name: String) -> String {
    format!("You can\u{2019}t send messages to {}.", f(&name))
}

/// Tapping call on someone who has not allowed calls from us, and a call that ended that way.
#[uniffi::export]
pub fn calls_not_allowed_text(name: String) -> String {
    format!(
        "{} hasn\u{2019}t allowed calls from you. You can still message them.",
        f(&name)
    )
}

/// Why the call button is closed, for a given composer state.
#[uniffi::export]
pub fn cant_call_text(name: String, composer: ComposerState) -> String {
    let f = f(&name);
    match composer {
        ComposerState::BlockedByMe => format!("You blocked {f}. Unblock to call."),
        ComposerState::BlockedMe => format!("Can\u{2019}t call: {f} blocked you"),
        ComposerState::NotContact => format!("{f} is no longer in your contacts."),
        ComposerState::Open => calls_not_allowed_text(name),
    }
}

/// Settings, Privacy, Unblocking.
#[uniffi::export]
pub fn unblock_note() -> String {
    "They are told you unblocked them and can message you again once they add you back: their app removed you from their contacts when you blocked them. Your chat history is kept either way.".into()
}

#[cfg(test)]
mod tests;
