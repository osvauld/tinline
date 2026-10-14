//! Group chat helpers shared by the group test files.

use std::sync::Arc;

use p2pcore::{GroupItemKind, GroupMessage, Node};

use super::{Peer, eventually};

/// Test installs that own a node and know their DID.
pub trait Member {
    fn node(&self) -> &Arc<Node>;
    fn did(&self) -> &str;
}

impl Member for Peer {
    fn node(&self) -> &Arc<Node> {
        &self.node
    }
    fn did(&self) -> &str {
        &self.did
    }
}

/// Default patience for group convergence.
pub const WAIT: u64 = 90;

/// `group_join`, unwrapped. GL: the product rides out transient network failures itself (the
/// fetch retries within its deadline, the join re-dials every 2 s while it waits), so the test-side
/// retry GF added is gone: any error here is a real failure.
pub fn join(p: &impl Member, link: String) -> String {
    p.node()
        .group_join(link)
        .unwrap_or_else(|e| panic!("join: {e:?}"))
}

/// Every message of group `g`, oldest first.
pub fn all(p: &impl Member, g: &str) -> Vec<GroupMessage> {
    let mut out = Vec::new();
    let mut before = None;
    loop {
        let page = p.node().group_messages(g.into(), before, 50).unwrap();
        let mut ms = page.messages;
        ms.reverse();
        out.extend(ms);
        match page.older {
            Some(o) => before = Some(o),
            None => break,
        }
    }
    out.reverse();
    out
}

/// `(author, text)` of every post, oldest first.
pub fn posts(p: &impl Member, g: &str) -> Vec<(String, String)> {
    all(p, g)
        .into_iter()
        .filter(|m| m.kind == GroupItemKind::Post)
        .map(|m| (m.author_did, m.text))
        .collect()
}

pub fn has_post(p: &impl Member, g: &str, did: &str, text: &str) -> bool {
    posts(p, g).iter().any(|(d, t)| d == did && t == text)
}

/// Sorted member DIDs of group `g` as `p` sees them.
pub fn members(p: &impl Member, g: &str) -> Vec<String> {
    let mut m: Vec<String> = p
        .node()
        .group_detail(g.into())
        .unwrap()
        .members
        .into_iter()
        .map(|m| m.did)
        .collect();
    m.sort();
    m
}

pub fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

/// Owner `o` (first host) creates a group and each of `ms` joins with an invite from `o`.
pub fn group_with(o: &Peer, ms: &[&Peer]) -> String {
    let g = o.node.group_create("Weekend hike".into()).unwrap();
    for m in ms {
        let tok = o.node.group_invite(g.clone()).unwrap();
        assert_eq!(join(*m, tok), g);
    }
    let want = sorted(
        std::iter::once(o.did.clone())
            .chain(ms.iter().map(|m| m.did.clone()))
            .collect(),
    );
    for p in std::iter::once(o).chain(ms.iter().copied()) {
        eventually(WAIT, "everyone sees every member", || {
            members(p, &g) == want
        });
    }
    g
}
