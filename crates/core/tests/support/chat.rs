//! Reading a 1:1 conversation back from a node.

use p2pcore::{DeliveryState, Message};

use super::Peer;

/// Every message of the conversation with `did`, newest day first (page order).
pub fn all_msgs(p: &Peer, did: &str) -> Vec<Message> {
    let mut out = Vec::new();
    let mut page = p.node.chat_day(did.to_string(), None).ok();
    while let Some(pg) = page {
        out.extend(pg.messages.clone());
        page = pg.older_day.and_then(|d| p.node.chat_day(did.to_string(), Some(d)).ok());
    }
    out
}

/// Every message, oldest first (the order the UI shows: day, then `(at, id)`).
pub fn all_msgs_oldest_first(p: &Peer, did: &str) -> Vec<Message> {
    let mut pages = Vec::new();
    let mut page = p.node.chat_day(did.to_string(), None).ok();
    while let Some(pg) = page {
        pages.push(pg.messages.clone());
        page = pg.older_day.and_then(|d| p.node.chat_day(did.to_string(), Some(d)).ok());
    }
    pages.into_iter().rev().flatten().collect()
}

/// Whether `p` holds message `id` of the conversation with `did`.
pub fn holds(p: &Peer, did: &str, id: &str) -> bool {
    all_msgs(p, did).iter().any(|m| m.id == id)
}

pub fn has(p: &Peer, did: &str, id: &str) -> bool {
    holds(p, did, id)
}

pub fn delivered(p: &Peer, did: &str, id: &str) -> bool {
    all_msgs(p, did).iter().any(|m| m.id == id && m.delivery == DeliveryState::Delivered)
}

pub fn ticks(p: &Peer, did: &str, id: &str) -> DeliveryState {
    all_msgs(p, did).into_iter().find(|m| m.id == id).expect("message held").delivery
}
