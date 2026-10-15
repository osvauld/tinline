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
mod tests {
    use super::*;

    fn doc(name: &str) -> (String, Vec<u8>) {
        (name.into(), VersionVector::default().encode())
    }

    #[test]
    fn legacy_peer_never_negotiates_group_contents_from_doc_names_alone() {
        assert!(!supported(&[doc(&format!(
            "group/{}/meta",
            "a".repeat(64)
        ))]));
        assert!(supported(&[doc(contents::CAPABILITY)]));
        assert!(!supported(&[(contents::CAPABILITY.into(), vec![255])]));
    }

    #[test]
    fn shared_catalog_is_bounded_meta_first_and_includes_both_domains() {
        let gid = "a".repeat(64);
        let group_meta = doc(&format!("group/{gid}/meta"));
        let group_day = doc(&format!("group/{gid}/2026-01-01"));
        let dm = doc(&format!("dm/{gid}/2026-01-01"));
        let result = catalog(
            vec![dm.clone(); MAX_OWN_DOCS],
            vec![group_meta.clone(), group_day.clone()],
        );
        // Test the saturation case separately from two-domain order.
        assert_eq!(result.len(), MAX_OWN_DOCS - 3);
        let mixed = catalog(
            vec![dm.clone()],
            vec![group_meta.clone(), group_day.clone()],
        );
        assert_eq!(mixed, vec![group_meta, dm, group_day]);
    }

    #[test]
    fn capability_plus_catalog_remains_compatible_with_legacy_hello_count_bound() {
        let data = catalog(
            (0..MAX_OWN_DOCS).map(|i| doc(&format!("dm:{i}"))).collect(),
            Vec::new(),
        );
        assert_eq!(data.len() + 3 + 4, MAX_OWN_DOCS + 4);
    }

    #[test]
    fn advisory_cache_is_bounded_and_does_not_evict_account_ack_state() {
        let mut peer = PeerChat::default();
        for i in 0..MAX_OWN_DOCS + 5 {
            remember(&mut peer, format!("doc:{i}"), VersionVector::default());
            assert!(peer.docs.len() <= MAX_OWN_DOCS);
        }
        assert!(peer.docs.contains_key(&format!("doc:{}", MAX_OWN_DOCS + 4)));
        assert!(peer.acks.is_empty());
    }
}
