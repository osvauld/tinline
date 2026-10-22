//! The chat list's mark (UniFFI): for every conversation whose last message is mine, the same
//! tick state the conversation shows (`ticks::tick_vis`), DMs and groups alike, decided here so
//! both apps only draw it.

use super::ticks::{TickVis, tick_vis};
use crate::Error;
use crate::group::api::GroupItemKind;
use crate::node::Node;

/// One conversation row's mark.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ListMark {
    /// The contact's DID (a DM) or the group id.
    pub conversation_id: String,
    pub group: bool,
    /// The last message the mark is for (a row whose last message changed drops its old mark).
    pub message_id: String,
    pub mark: TickVis,
}

#[uniffi::export]
impl Node {
    /// The mark of every conversation (DM and group) whose last message is mine. Blocking; re-read
    /// with the list (`on_chat_changed`, `on_message_changed`, `on_group_changed`). A part that
    /// cannot be read (no group store yet) is left out, never guessed.
    pub fn chat_list_marks(&self) -> Result<Vec<ListMark>, Error> {
        let mut out = Vec::new();
        if let Ok(rows) = self.inner.chat_list_receipts() {
            for r in rows {
                let not_delivered = self
                    .not_delivered_reason(r.peer_did.clone(), r.status.message_id.clone())
                    .is_some();
                if let Some(mark) = tick_vis(Some(r.status.tick), not_delivered) {
                    out.push(ListMark {
                        conversation_id: r.peer_did,
                        group: false,
                        message_id: r.status.message_id,
                        mark,
                    });
                }
            }
        }
        if let Ok(core) = self.inner.group_core() {
            for s in self.groups().unwrap_or_default() {
                let Some(m) = s
                    .last
                    .filter(|m| m.outgoing && m.kind == GroupItemKind::Post)
                else {
                    continue;
                };
                let mark = match m.delivery {
                    crate::group::api::GroupDelivery::Waiting | crate::group::api::GroupDelivery::Failed => TickVis::Slack,
                    crate::group::api::GroupDelivery::Delivered => core
                        .member_receipts_range(&s.id, m.at as i64, m.at as i64, true)
                        .ok()
                        .and_then(|v| v.into_iter().find(|st| st.message_id == m.id))
                        .and_then(|st| tick_vis(Some(st.tick), false))
                        .unwrap_or(TickVis::One),
                };
                out.push(ListMark {
                    conversation_id: s.id,
                    group: true,
                    message_id: m.id,
                    mark,
                });
            }
        }
        Ok(out)
    }
}
