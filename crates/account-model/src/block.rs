//! Block notices' effect on the stored state. p2pcore owns signing, delivering and checking them.

use crate::store::State;

/// U20, the deletion request every block notice carries: the blocked person's app deletes its
/// contact entry for the blocker (address, grant, device list) unless it was added after the
/// notice was signed. The chat history is kept, read-only (`chat_contact`). The one place that
/// decides what a deletion request deletes. Returns whether a contact went.
pub fn apply_block_deletion_request(st: &mut State, from: &str, at_ms: u64) -> bool {
    let n = st.contacts.len();
    st.contacts.retain(|c| !(c.did == from && c.added_at.saturating_mul(1000) <= at_ms));
    st.contacts.len() != n
}
