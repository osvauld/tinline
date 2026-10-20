//! Bounded inventories/cache for the optional group-content extension to self/3.
//! Peers without this Hello marker keep metadata-only sync. Archive frames have
//! their own additional negotiation marker.
use super::*;
use crate::group::own::contents;

pub(super) fn supported(docs: &[(String, Vec<u8>)]) -> bool {
    docs.iter().any(|(name, vv)| {
        name == contents::CAPABILITY
            && VersionVector::decode(vv)
                .ok()
                .is_some_and(|v| v == VersionVector::default())
    })
}

pub(super) fn catalog(
    chat: Vec<(String, Vec<u8>)>,
    groups: Vec<(String, Vec<u8>)>,
) -> Vec<(String, Vec<u8>)> {
    // Reserve three slots for content/history/receipt markers. With four fixed account
    // docs, Hello stays inside older peers' MAX_OWN_DOCS + 4.
    let limit = MAX_OWN_DOCS - 3;
    let (meta, days): (Vec<_>, Vec<_>) = groups.into_iter().partition(|(name, _)| {
        matches!(
            contents::parse_doc(name),
            Some((_, crate::group::doc::DocId::Meta))
        )
    });
    let mut out: Vec<_> = meta.into_iter().take(limit).collect();
    let (mut chat, mut days) = (chat.into_iter(), days.into_iter());
    while out.len() < limit {
        let (a, b) = (chat.next(), days.next());
        if a.is_none() && b.is_none() {
            break;
        }
        for item in [a, b].into_iter().flatten() {
            if out.len() == limit {
                break;
            }
            out.push(item);
        }
    }
    out
}

pub(super) fn remember(peer: &mut PeerChat, name: String, vv: VersionVector) {
    // This is only advisory per-session peer knowledge, not persisted data or a
    // destination receipt. Forgetting old entries can cause safe duplicate sends.
    while !peer.docs.contains_key(&name) && peer.docs.len() >= MAX_OWN_DOCS {
        let victim = peer
            .docs
            .keys()
            .min_by_key(|name| {
                if let Some((_, crate::group::doc::DocId::Day(day))) = contents::parse_doc(name) {
                    (0, day, (*name).clone())
                } else if let Some((_, day)) = parse_doc(name) {
                    (1, day, (*name).clone())
                } else {
                    (2, String::new(), (*name).clone())
                }
            })
            .cloned();
        let Some(victim) = victim else {
            break;
        };
        peer.docs.remove(&victim);
    }
    peer.docs
        .entry(name)
        .and_modify(|v| v.merge(&vv))
        .or_insert(vv);
}

#[cfg(test)]
mod wire_tests;

#[cfg(test)]
mod tests;
