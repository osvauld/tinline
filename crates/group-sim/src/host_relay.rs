//! W2-R: rotation notes relayed by the group host (user decisions U1, U2).
//!
//! Simulation only. A `RotationNote` is a stand-in for a succession permit: the owner DID signs
//! (owner, audience, endpoint, seq) with its real Ed25519 DID key. It is NOT the succession
//! verifier (G-D), carries no device path, and no future endpoint is derived from anything:
//! every endpoint is a fresh fixture string. Time is not modelled (no expiry/grace, G-TIME).
//! Membership is a fixed roster fixture; group authorization stays in `authority.rs`.
//! Endpoint strings: group endpoints start with `grp-`, private endpoints with `PRIVATE-`, so
//! audience leaks can be found by scanning bytes.
//!
//! R1b (U7: only the host knows members' group endpoints): a member's `GroupView` accepts only
//! its own and the host's notes (`Reject::NotHost` otherwise); the host relays its own note
//! to members but never a member's note; a returning member retries its own notes to the host
//! and pulls the host's; recovery after a host rotation is the host's retry (`host_push`), not
//! member forwarding. Removed by R1b, with their current-design coverage:
//! - `member_rotation_is_relayed_to_online_members_and_offline_member_catches_up_from_host`,
//!   `two_members_rotated_while_apart_recover_each_other_via_host`: `host_only.rs`
//!   `member_rotation_note_goes_to_host_only_and_host_routes_to_new_endpoint`, `multi_host.rs`
//!   `member_rotation_note_reaches_every_host_and_hosts_repair_each_other` and
//!   `host_and_member_both_rotated_while_apart_recover_via_another_host`;
//! - `host_rotation_reaches_offline_member_through_verified_member_forwarding`: `host_only.rs`
//!   `host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry` and
//!   `host_overlap_window_candidate_recovers_member_only_within_window`, `multi_host.rs`
//!   `rotated_host_reconnects_members_at_their_current_endpoints_without_overlap`;
//! - `host_cannot_substitute_endpoint_in_relayed_note` (the host no longer relays member
//!   notes to members): `multi_host.rs` `member_rotation_note_reaches_every_host_and_hosts_repair_each_other`
//!   (note altered in host-to-host relay refused) and the forged-host-note cases of
//!   `host_only.rs` `host_rotation_reaches_online_members_directly_and_offline_member_by_host_retry`;
//! - `host_and_all_forwarders_unreachable_means_no_recovery_without_reshare`: `host_only.rs`
//!   `host_and_member_both_rotated_while_apart_lose_each_other`.
use super::*;
use identity::{public_key_from_did, verify};

const NOTE_DOMAIN: &[u8] = b"tinline/group-sim/rotation-note/not-production/v1\0";
const HOST: usize = 0;
const A: usize = 1;
const C: usize = 2;
const D: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum NoteAudience {
    Group(String),
    Private,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NoteClaims {
    owner: String,
    audience: NoteAudience,
    endpoint: String,
    seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RotationNote {
    claims: NoteClaims,
    signature: Vec<u8>,
}
impl RotationNote {
    fn signed_bytes(claims: &NoteClaims) -> Vec<u8> {
        [NOTE_DOMAIN, &serde_json::to_vec(claims).unwrap()].concat()
    }
    fn sign(id: &Identity, audience: NoteAudience, endpoint: &str, seq: u64) -> Self {
        let claims = NoteClaims {
            owner: id.did().into(),
            audience,
            endpoint: endpoint.into(),
            seq,
        };
        let signature = id.sign(&Self::signed_bytes(&claims)).to_vec();
        Self { claims, signature }
    }
    fn verify(&self) -> bool {
        let Some(key) = public_key_from_did(&self.claims.owner) else {
            return false;
        };
        let Ok(sig) = <[u8; 64]>::try_from(self.signature.as_slice()) else {
            return false;
        };
        verify(&key, &Self::signed_bytes(&self.claims), &sig)
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Reject {
    Malformed,
    PrivateAudience,
    WrongGroup,
    NotMember,
    NotHost,
    BadSignature,
    Frozen,
    Replay,
    Stale,
    ForkSameSeq,
    Gap,
}
// Open gate G-HISTORY (Q3): may a receiver skip intermediate notes?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GapPolicy {
    AcceptGap,
    RequireContiguous,
}
// Open gate G-FORK (Q4.1): two different notes with the same (owner, seq).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForkPolicy {
    KeepFirst,
    Freeze,
}
// Open gate G-HISTORY: what the host (or a forwarder) sends on catch-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CatchUp {
    FullHistory,
    LatestOnly,
}
#[derive(Clone, Copy, Debug)]
struct Policy {
    gap: GapPolicy,
    fork: ForkPolicy,
    catch_up: CatchUp,
}
// Invariant tests run under every candidate combination: they must not depend on the choice.
fn all_policies() -> Vec<Policy> {
    let mut out = vec![];
    for gap in [GapPolicy::AcceptGap, GapPolicy::RequireContiguous] {
        for fork in [ForkPolicy::KeepFirst, ForkPolicy::Freeze] {
            for catch_up in [CatchUp::FullHistory, CatchUp::LatestOnly] {
                out.push(Policy {
                    gap,
                    fork,
                    catch_up,
                });
            }
        }
    }
    out
}

// Group-audience directory. The host's holds every member's notes; a member's (U7) holds only
// its own and the host's. Serializable so that rejection tests compare full bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct GroupView {
    group: String,
    roster: BTreeSet<String>,
    // U7: `Some({self, host})` on a member, `None` on the host.
    accept_only: Option<BTreeSet<String>>,
    history: BTreeMap<String, Vec<RotationNote>>,
    frozen: BTreeSet<String>,
}
impl GroupView {
    fn current(&self, did: &str) -> Option<&RotationNote> {
        self.history.get(did).and_then(|h| h.last())
    }
    fn route(&self, did: &str) -> Option<&RotationNote> {
        if self.frozen.contains(did) {
            return None;
        }
        self.current(did)
    }
    fn accept(&mut self, policy: Policy, bytes: &[u8]) -> Result<RotationNote, Reject> {
        let note: RotationNote = serde_json::from_slice(bytes).map_err(|_| Reject::Malformed)?;
        let c = &note.claims;
        match &c.audience {
            NoteAudience::Private => return Err(Reject::PrivateAudience),
            NoteAudience::Group(g) if g != &self.group => return Err(Reject::WrongGroup),
            NoteAudience::Group(_) => {}
        }
        if !self.roster.contains(&c.owner) {
            return Err(Reject::NotMember);
        }
        if self
            .accept_only
            .as_ref()
            .is_some_and(|o| !o.contains(&c.owner))
        {
            return Err(Reject::NotHost);
        }
        if !note.verify() {
            return Err(Reject::BadSignature);
        }
        if self.frozen.contains(&c.owner) {
            return Err(Reject::Frozen);
        }
        let history = self.history.get(&c.owner);
        if history.is_some_and(|h| h.contains(&note)) {
            return Err(Reject::Replay);
        }
        let last_seq = history.and_then(|h| h.last()).map(|n| n.claims.seq);
        match last_seq {
            Some(last) if c.seq < last => return Err(Reject::Stale),
            Some(last) if c.seq == last => {
                if policy.fork == ForkPolicy::Freeze {
                    // Candidate: the fork itself is state (freezes the path).
                    self.frozen.insert(c.owner.clone());
                }
                return Err(Reject::ForkSameSeq);
            }
            _ => {}
        }
        if policy.gap == GapPolicy::RequireContiguous && c.seq != last_seq.unwrap_or(0) + 1 {
            return Err(Reject::Gap);
        }
        self.history
            .entry(c.owner.clone())
            .or_default()
            .push(note.clone());
        Ok(note)
    }
    // One owner's notes as offered on a retry/catch-up (G-HISTORY).
    fn export_owner(&self, mode: CatchUp, owner: &str) -> Vec<Vec<u8>> {
        let h = self.history.get(owner).map(Vec::as_slice).unwrap_or(&[]);
        match mode {
            CatchUp::FullHistory => h.iter().map(RotationNote::wire).collect(),
            CatchUp::LatestOnly => h.last().map(RotationNote::wire).into_iter().collect(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum RouteErr {
    SelfOffline,
    NoBinding,
    EndpointGone,
}
#[derive(Clone, Debug)]
struct Frame {
    from: String,
    to: String,
    bytes: Vec<u8>,
}
struct Node {
    name: &'static str,
    author: Author,
    did: String,
    view: GroupView,
    group_seq: u64,
    private_seq: u64,
    private_endpoint: String,
    private_contacts: BTreeSet<String>,
    // Personal block list: private-audience only (U5). Never consulted by group code.
    blocked: BTreeSet<String>,
    online: bool,
    // Bytes this node sent in its role as group host (relay + catch-up).
    relayed: Vec<Vec<u8>>,
}
struct World {
    policy: Policy,
    group: String,
    nodes: Vec<Node>,
    live: BTreeMap<String, String>, // listening endpoint -> owner DID
    wire: Vec<Frame>,               // every frame sent over a group-audience link
    private_wire: Vec<Frame>,       // private-audience notes, sent directly to contacts
    private_endpoints_ever: BTreeSet<String>,
}
fn world(policy: Policy) -> World {
    let (g, a, b, c) = signed_fixture();
    let d = Author::new(4, &g.id());
    let group = g.id();
    let authors = vec![("host", b), ("alice", a), ("carol", c), ("dave", d)];
    let roster: BTreeSet<String> = authors
        .iter()
        .map(|(_, x)| x.id.did().to_string())
        .collect();
    let mut w = World {
        policy,
        group: group.clone(),
        nodes: vec![],
        live: BTreeMap::new(),
        wire: vec![],
        private_wire: vec![],
        private_endpoints_ever: BTreeSet::new(),
    };
    let mut bootstrap = vec![];
    let host_did = authors[HOST].1.id.did().to_string();
    for (name, author) in authors {
        let did = author.id.did().to_string();
        let ep = format!("grp-{name}-1");
        bootstrap.push(
            RotationNote::sign(&author.id, NoteAudience::Group(group.clone()), &ep, 1).wire(),
        );
        let private_endpoint = format!("PRIVATE-{name}-1");
        w.live.insert(ep, did.clone());
        w.live.insert(private_endpoint.clone(), did.clone());
        w.private_endpoints_ever.insert(private_endpoint.clone());
        w.nodes.push(Node {
            name,
            author,
            did: did.clone(),
            view: GroupView {
                group: group.clone(),
                roster: roster.clone(),
                accept_only: (did != host_did).then(|| [did.clone(), host_did.clone()].into()),
                history: BTreeMap::new(),
                frozen: BTreeSet::new(),
            },
            group_seq: 1,
            private_seq: 1,
            private_endpoint,
            private_contacts: roster.iter().filter(|x| **x != did).cloned().collect(),
            blocked: BTreeSet::new(),
            online: true,
            relayed: vec![],
        });
    }
    // Join bootstrap (invite/ticket) is out of scope: the host's view starts with every seq-1
    // note, a member's with its own and the host's (U7).
    for node in &mut w.nodes {
        for b in &bootstrap {
            match node.view.accept(policy, b) {
                Ok(_) | Err(Reject::NotHost) => {}
                Err(e) => panic!("bootstrap note refused: {e:?}"),
            }
        }
    }
    w
}
impl World {
    fn did(&self, i: usize) -> String {
        self.nodes[i].did.clone()
    }
    fn host_snapshot(&self) -> Vec<u8> {
        let h = &self.nodes[HOST];
        serde_json::to_vec(&(&h.view, &h.relayed)).unwrap()
    }
    fn view_snapshot(&self, i: usize) -> Vec<u8> {
        serde_json::to_vec(&self.nodes[i].view).unwrap()
    }
    fn known_endpoint(&self, viewer: usize, target: usize) -> Option<String> {
        self.nodes[viewer]
            .view
            .current(&self.nodes[target].did)
            .map(|n| n.claims.endpoint.clone())
    }
    fn reach(&self, from: usize, to: usize) -> Result<(), RouteErr> {
        if !self.nodes[from].online {
            return Err(RouteErr::SelfOffline);
        }
        let did = &self.nodes[to].did;
        let Some(note) = self.nodes[from].view.route(did) else {
            return Err(RouteErr::NoBinding);
        };
        if self.live.get(&note.claims.endpoint) != Some(did) || !self.nodes[to].online {
            return Err(RouteErr::EndpointGone);
        }
        Ok(())
    }
    fn send(&mut self, from: usize, to: usize, bytes: &[u8]) {
        self.wire.push(Frame {
            from: self.did(from),
            to: self.did(to),
            bytes: bytes.to_vec(),
        });
    }
    // Raw adversarial delivery (no route check) straight into `to`'s group view.
    fn inject(&mut self, from: usize, to: usize, bytes: &[u8]) -> Result<RotationNote, Reject> {
        self.send(from, to, bytes);
        let policy = self.policy;
        self.nodes[to].view.accept(policy, bytes)
    }
    // Periodic group-endpoint rotation (never triggered by a block). Old endpoint stops
    // listening; new one is a fresh fixture string, not derived from anything.
    fn rotate_group(&mut self, i: usize) -> Vec<u8> {
        let n = &mut self.nodes[i];
        let old = n.view.current(&n.did).unwrap().claims.endpoint.clone();
        n.group_seq += 1;
        let ep = format!("grp-{}-{}", n.name, n.group_seq);
        let note = RotationNote::sign(
            &n.author.id,
            NoteAudience::Group(self.group.clone()),
            &ep,
            n.group_seq,
        );
        let bytes = note.wire();
        n.view.accept(self.policy, &bytes).unwrap();
        self.live.remove(&old);
        self.live.insert(ep, n.did.clone());
        bytes
    }
    // Private-audience rotation: sent directly to unblocked contacts, never via the host.
    fn rotate_private(&mut self, i: usize) -> Vec<String> {
        let n = &mut self.nodes[i];
        self.live.remove(&n.private_endpoint);
        n.private_seq += 1;
        n.private_endpoint = format!("PRIVATE-{}-{}", n.name, n.private_seq);
        self.live.insert(n.private_endpoint.clone(), n.did.clone());
        self.private_endpoints_ever
            .insert(n.private_endpoint.clone());
        let note = RotationNote::sign(
            &n.author.id,
            NoteAudience::Private,
            &n.private_endpoint,
            n.private_seq,
        );
        let recipients: Vec<String> = n
            .private_contacts
            .iter()
            .filter(|c| !n.blocked.contains(*c))
            .cloned()
            .collect();
        for to in &recipients {
            self.private_wire.push(Frame {
                from: n.did.clone(),
                to: to.clone(),
                bytes: note.wire(),
            });
        }
        recipients
    }
    // Host relays its OWN note to every member it can reach. U7: never a member's note.
    fn fanout(&mut self, bytes: &[u8]) -> Vec<usize> {
        let note: RotationNote = serde_json::from_slice(bytes).unwrap();
        assert_eq!(
            note.claims.owner,
            self.did(HOST),
            "U7: host relays only its own note"
        );
        let mut delivered = vec![];
        for j in 1..self.nodes.len() {
            if self.reach(HOST, j).is_err() {
                continue;
            }
            self.send(HOST, j, bytes);
            self.nodes[HOST].relayed.push(bytes.to_vec());
            let policy = self.policy;
            if self.nodes[j].view.accept(policy, bytes).is_ok() {
                delivered.push(j);
            }
        }
        delivered
    }
    // A member sends its own group note to the host only (U1 group audience, U7); the host
    // sends its own note to the members. Returns the members that accepted a host note.
    fn announce(&mut self, i: usize, bytes: &[u8]) -> Result<Vec<usize>, RouteErr> {
        if i == HOST {
            return Ok(self.fanout(bytes));
        }
        self.reach(i, HOST)?;
        self.send(i, HOST, bytes);
        let policy = self.policy;
        let _ = self.nodes[HOST].view.accept(policy, bytes);
        Ok(vec![])
    }
    // U7 host retry: the host dials a member at the member's current group endpoint (only the
    // host knows it) and hands over its own current note. Returns whether it was accepted.
    fn host_push(&mut self, j: usize) -> Result<bool, RouteErr> {
        self.reach(HOST, j)?;
        let bytes = self.nodes[HOST]
            .view
            .current(&self.did(HOST))
            .unwrap()
            .wire();
        self.send(HOST, j, &bytes);
        self.nodes[HOST].relayed.push(bytes.clone());
        let policy = self.policy;
        Ok(self.nodes[j].view.accept(policy, &bytes).is_ok())
    }
    // Member returns: its live side retries its own notes to the host (latest only or full
    // history, G-HISTORY) and pulls the host's own notes. Returns (accepted by the host,
    // accepted by the member).
    fn catch_up(&mut self, i: usize) -> Result<(usize, usize), RouteErr> {
        self.reach(i, HOST)?;
        let mode = self.policy.catch_up;
        let own = self.nodes[i].view.export_owner(mode, &self.did(i));
        let mut to_host = 0;
        for bytes in own {
            self.send(i, HOST, &bytes);
            let policy = self.policy;
            if self.nodes[HOST].view.accept(policy, &bytes).is_ok() {
                to_host += 1;
            }
        }
        let notes = self.nodes[HOST].view.export_owner(mode, &self.did(HOST));
        let mut to_member = 0;
        for bytes in notes {
            self.send(HOST, i, &bytes);
            self.nodes[HOST].relayed.push(bytes.clone());
            let policy = self.policy;
            if self.nodes[i].view.accept(policy, &bytes).is_ok() {
                to_member += 1;
            }
        }
        Ok((to_host, to_member))
    }
    fn group_bytes_scan(&self) -> Vec<Vec<u8>> {
        let mut hay = vec![self.host_snapshot()];
        hay.extend(self.wire.iter().map(|f| f.bytes.clone()));
        hay.extend(
            self.nodes
                .iter()
                .map(|n| serde_json::to_vec(&n.view).unwrap()),
        );
        hay
    }
}
fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

// KIND: invariant
#[test]
fn private_endpoints_never_appear_in_host_state_or_any_group_relayed_bytes() {
    for policy in all_policies() {
        let mut w = world(policy);
        // A full mix of group and private rotations, an offline catch-up and a host rotation.
        w.nodes[D].online = false;
        for i in [A, C, HOST] {
            w.rotate_private(i);
            let n = w.rotate_group(i);
            w.announce(i, &n).unwrap();
            w.rotate_private(i);
        }
        w.nodes[D].online = true;
        // U7: the host's retry reaches dave at his still-live endpoint (no member forwarding).
        assert_eq!(w.host_push(D), Ok(true));
        w.catch_up(D).unwrap();
        w.rotate_private(D);
        let n = w.rotate_group(D);
        w.announce(D, &n).unwrap();
        assert!(!w.wire.is_empty() && !w.nodes[HOST].relayed.is_empty());
        // Positive control: the private endpoints do exist and the scan can see them.
        assert_eq!(w.private_endpoints_ever.len(), 4 + 7);
        for ep in &w.private_endpoints_ever {
            assert!(w.private_wire.iter().any(|f| contains(&f.bytes, ep)) || ep.ends_with("-1"));
        }
        let hay = w.group_bytes_scan();
        for bytes in &hay {
            assert!(!contains(bytes, "PRIVATE-"), "{policy:?}");
            for ep in &w.private_endpoints_ever {
                assert!(!contains(bytes, ep));
            }
        }
        // A buggy/hostile member pushes its private note at the host: refused, and host
        // state plus everything the host relays stays free of it. (The inbound frame itself
        // carries the leak; the host cannot un-send a sender's mistake.)
        let leak = RotationNote::sign(
            &w.nodes[C].author.id,
            NoteAudience::Private,
            &w.nodes[C].private_endpoint.clone(),
            w.nodes[C].private_seq,
        )
        .wire();
        let before = w.host_snapshot();
        assert_eq!(w.inject(C, HOST, &leak), Err(Reject::PrivateAudience));
        assert_eq!(w.host_snapshot(), before);
        assert!(!contains(&w.host_snapshot(), "PRIVATE-"));
        assert!(
            w.nodes[HOST]
                .relayed
                .iter()
                .all(|r| !contains(r, "PRIVATE-"))
        );
    }
}

// KIND: invariant
// R1b (U7): the host accepts members' group notes; a member accepts only the host's. Every
// malformed, forged, bit-flipped, truncated, stale, replayed, wrong-owner, non-member,
// private-audience or foreign note leaves the host's state (for alice's notes, sent by a
// member) and a member's view (for the host's notes, sent by a faulty relay) byte-identical.
// A genuine note of another member is refused by a member (`NotHost`), view unchanged.
#[test]
fn forged_stale_replayed_wrong_owner_private_and_foreign_notes_leave_host_and_member_unchanged() {
    for policy in all_policies() {
        let mut w = world(policy);
        let n2 = w.rotate_group(A);
        assert_eq!(w.announce(A, &n2), Ok(vec![]));
        let h2 = w.rotate_group(HOST);
        assert_eq!(w.announce(HOST, &h2), Ok(vec![A, C, D]));
        // (owner of the notes, receiver, sender, current note)
        for (owner, to, from, current) in [(A, HOST, D, n2.clone()), (HOST, C, HOST, h2)] {
            let id = w.nodes[owner].author.id.clone();
            let name = w.nodes[owner].name;
            let group = NoteAudience::Group(w.group.clone());
            let n1 = RotationNote::sign(&id, group.clone(), &format!("grp-{name}-1"), 1).wire();
            let mut forged: RotationNote = serde_json::from_slice(&current).unwrap();
            forged.claims.seq = 3;
            forged.claims.endpoint = format!("grp-{name}-hijack");
            let mut flipped: RotationNote = serde_json::from_slice(&current).unwrap();
            flipped.signature[7] ^= 1;
            let mut short_sig: RotationNote = serde_json::from_slice(&current).unwrap();
            short_sig.signature.truncate(63);
            let stale = RotationNote::sign(&id, group.clone(), "grp-rollback", 1).wire();
            let mut wrong_owner =
                RotationNote::sign(&w.nodes[D].author.id, group.clone(), "grp-x", 3);
            wrong_owner.claims.owner = w.did(owner);
            let outsider = Author::new(9, &w.group);
            let not_member = RotationNote::sign(&outsider.id, group.clone(), "grp-out-1", 1).wire();
            let private = RotationNote::sign(&id, NoteAudience::Private, "PRIVATE-x-9", 3).wire();
            let foreign =
                RotationNote::sign(&id, NoteAudience::Group("other".into()), "grp-o", 3).wire();
            let cases: Vec<(&str, Vec<u8>, Reject)> = vec![
                ("forged", forged.wire(), Reject::BadSignature),
                ("bit-flip", flipped.wire(), Reject::BadSignature),
                ("short-sig", short_sig.wire(), Reject::BadSignature),
                ("stale", stale, Reject::Stale),
                ("replay-current", current.clone(), Reject::Replay),
                ("replay-old", n1, Reject::Replay),
                ("wrong-owner", wrong_owner.wire(), Reject::BadSignature),
                ("not-member", not_member, Reject::NotMember),
                ("private", private, Reject::PrivateAudience),
                ("wrong-group", foreign, Reject::WrongGroup),
                ("malformed", b"{\"claims\":1}".to_vec(), Reject::Malformed),
            ];
            for (case, bytes, want) in cases {
                let snap = |w: &World| {
                    if to == HOST {
                        w.host_snapshot()
                    } else {
                        w.view_snapshot(to)
                    }
                };
                let before = snap(&w);
                assert_eq!(
                    w.inject(from, to, &bytes),
                    Err(want),
                    "{name} {case} {policy:?}"
                );
                assert_eq!(snap(&w), before, "{name} {case} mutated node {to}");
            }
        }
        let before = w.view_snapshot(C);
        assert_eq!(w.inject(HOST, C, &n2), Err(Reject::NotHost));
        assert_eq!(w.view_snapshot(C), before);
        // Positive controls on the same populated state.
        let n3 = w.rotate_group(A);
        assert_eq!(w.announce(A, &n3), Ok(vec![]));
        assert_eq!(w.known_endpoint(HOST, A), Some("grp-alice-3".into()));
        assert_eq!(w.known_endpoint(C, A), None);
        let h3 = w.rotate_group(HOST);
        assert_eq!(w.announce(HOST, &h3), Ok(vec![A, C, D]));
        assert_eq!(w.known_endpoint(C, HOST), Some("grp-host-3".into()));
    }
}

// KIND: invariant
// R1b (U7): Alice personally blocks Carol. The host still holds both their new group
// endpoints and reaches both; Carol still receives the host's rotation; no member holds
// another member's group endpoint in either run; every group view and the host relay log are
// byte-identical with and without the block. Only the private audience differs: Carol is
// left out of Alice's private note.
#[test]
fn personal_block_does_not_stop_group_notes_or_change_group_state() {
    for policy in all_policies() {
        let run = |block: bool| {
            let mut w = world(policy);
            if block {
                // Alice personally blocks carol (private audience only, U5 revised).
                let c = w.did(C);
                w.nodes[A].blocked.insert(c);
            }
            let private_recipients = w.rotate_private(A);
            for i in [A, C] {
                let n = w.rotate_group(i);
                assert_eq!(w.announce(i, &n), Ok(vec![]));
            }
            let n = w.rotate_group(HOST);
            let delivered = w.announce(HOST, &n).unwrap();
            let views: Vec<Vec<u8>> = (0..4).map(|i| w.view_snapshot(i)).collect();
            (w.host_snapshot(), views, delivered, private_recipients, w)
        };
        let (host_plain, views_plain, del_plain, priv_plain, _) = run(false);
        let (host_block, views_block, del_block, priv_block, w) = run(true);
        assert_eq!(del_block, vec![A, C, D]);
        assert_eq!(w.known_endpoint(C, HOST), Some("grp-host-2".into()));
        assert_eq!(w.known_endpoint(HOST, A), Some("grp-alice-2".into()));
        assert_eq!(w.known_endpoint(HOST, C), Some("grp-carol-2".into()));
        for (x, y) in [(C, HOST), (HOST, C), (A, HOST), (HOST, A)] {
            assert_eq!(w.reach(x, y), Ok(()));
        }
        assert_eq!(w.known_endpoint(C, A), None);
        assert_eq!(w.known_endpoint(A, C), None);
        assert_eq!(w.reach(C, A), Err(RouteErr::NoBinding));
        // Group state (roster, directories, host relay log) is byte-identical with the block.
        assert_eq!(del_plain, del_block);
        assert_eq!(host_plain, host_block);
        assert_eq!(views_plain, views_block);
        // Only the private audience differs: carol is excluded from alice's private note.
        assert!(priv_plain.contains(&w.did(C)));
        assert!(!priv_block.contains(&w.did(C)));
        assert_eq!(priv_block.len() + 1, priv_plain.len());
    }
}

// KIND: candidate-policy G-HISTORY
// R1b (U7): only the host holds members' group notes, so the history question is what a
// member's retry sends the host after the host missed several of its rotations. Alice rotates
// three times (seq 2..4) while the host is offline; when it returns, her retry sends her latest
// note only, or her full history (seq 1..4). A strict (contiguous) host cannot use a
// latest-only retry and keeps routing alice to her retired endpoint. Members' views are
// untouched in every case.
#[test]
fn catch_up_latest_only_vs_full_history_under_gap_policies() {
    let mut results = vec![];
    for policy in all_policies()
        .into_iter()
        .filter(|p| p.fork == ForkPolicy::KeepFirst)
    {
        let mut w = world(policy);
        w.nodes[HOST].online = false;
        for _ in 0..3 {
            let n = w.rotate_group(A);
            assert_eq!(w.announce(A, &n), Err(RouteErr::EndpointGone));
        }
        w.nodes[HOST].online = true;
        let host_before = w.view_snapshot(HOST);
        let members_before: Vec<Vec<u8>> = [C, D].iter().map(|&i| w.view_snapshot(i)).collect();
        let wire_before = w.wire.len();
        let (accepted, _) = w.catch_up(A).unwrap();
        let sent: Vec<&Frame> = w.wire[wire_before..]
            .iter()
            .filter(|f| f.from == w.did(A) && f.to == w.did(HOST))
            .collect();
        let bytes: usize = sent.iter().map(|f| f.bytes.len()).sum();
        let routed = w.reach(HOST, A);
        match (policy.catch_up, policy.gap) {
            (CatchUp::LatestOnly, GapPolicy::RequireContiguous) => {
                assert_eq!((accepted, sent.len()), (0, 1));
                assert_eq!(w.view_snapshot(HOST), host_before);
                assert_eq!(routed, Err(RouteErr::EndpointGone));
            }
            (CatchUp::LatestOnly, GapPolicy::AcceptGap) => {
                assert_eq!((accepted, sent.len()), (1, 1));
                assert_eq!(routed, Ok(()));
            }
            (CatchUp::FullHistory, _) => {
                // seq 1 is a replay at the host; seq 2..4 are new.
                assert_eq!((accepted, sent.len()), (3, 4));
                assert_eq!(routed, Ok(()));
            }
        }
        let members_after: Vec<Vec<u8>> = [C, D].iter().map(|&i| w.view_snapshot(i)).collect();
        assert_eq!(members_after, members_before);
        results.push((policy.catch_up, policy.gap, sent.len(), bytes));
    }
    // Full history costs more bytes on every retry and grows with rotation count.
    let latest = results
        .iter()
        .find(|r| r.0 == CatchUp::LatestOnly)
        .unwrap()
        .3;
    let full = results
        .iter()
        .find(|r| r.0 == CatchUp::FullHistory)
        .unwrap()
        .3;
    assert!(full > latest);
}

// KIND: candidate-policy G-FORK
// R1b: the two views are the group's two hosts (multi-host is decided; under U7 a member
// never holds alice's note), receiving the honest and the forked seq-2 note in opposite order.
#[test]
fn same_seq_fork_keep_first_diverges_by_arrival_order_freeze_converges_but_blocks_routing() {
    for fork in [ForkPolicy::KeepFirst, ForkPolicy::Freeze] {
        let policy = Policy {
            gap: GapPolicy::AcceptGap,
            fork,
            catch_up: CatchUp::FullHistory,
        };
        let w = world(policy);
        // A linked device / mnemonic clone of alice signs a different seq-2 note.
        let clone = Author::new(1, &w.group);
        assert_eq!(clone.id.did(), w.did(A));
        let g = NoteAudience::Group(w.group.clone());
        let honest = RotationNote::sign(&w.nodes[A].author.id, g.clone(), "grp-alice-2", 2).wire();
        let forked = RotationNote::sign(&clone.id, g.clone(), "grp-alice-clone-2", 2).wire();
        let mut host = w.nodes[HOST].view.clone();
        let mut host2 = w.nodes[HOST].view.clone();
        assert!(host.accept(policy, &honest).is_ok());
        assert_eq!(host.accept(policy, &forked), Err(Reject::ForkSameSeq));
        assert!(host2.accept(policy, &forked).is_ok());
        assert_eq!(host2.accept(policy, &honest), Err(Reject::ForkSameSeq));
        let a = w.did(A);
        match fork {
            ForkPolicy::KeepFirst => {
                // Arrival order picks the winner: the two hosts route alice differently.
                assert_eq!(host.route(&a).unwrap().claims.endpoint, "grp-alice-2");
                assert_eq!(
                    host2.route(&a).unwrap().claims.endpoint,
                    "grp-alice-clone-2"
                );
            }
            ForkPolicy::Freeze => {
                // Order-independent, but alice's group path is frozen everywhere: any
                // mnemonic holder can DoS it. Later honest notes are refused too.
                assert!(host.route(&a).is_none() && host2.route(&a).is_none());
                assert_eq!(host.frozen, host2.frozen);
                let n3 = RotationNote::sign(&w.nodes[A].author.id, g, "grp-alice-3", 3).wire();
                let before = serde_json::to_vec(&host).unwrap();
                assert_eq!(host.accept(policy, &n3), Err(Reject::Frozen));
                assert_eq!(serde_json::to_vec(&host).unwrap(), before);
            }
        }
    }
}

// KIND: limitation
// R1b (U7): a mnemonic clone of alice signs a higher-seq group note; the host cannot tell it
// from alice's and now routes alice's group traffic to the clone. Members never held alice's
// group endpoint, so the effect is confined to what hosts route. Alice's next honest note
// then looks like a fork and is refused; nothing here can tell which device is the owner's.
#[test]
fn mnemonic_clone_with_higher_seq_replaces_honest_group_endpoint_at_the_host() {
    let mut w = world(all_policies()[0]);
    let clone = Author::new(1, &w.group);
    let g = NoteAudience::Group(w.group.clone());
    let n2 = w.rotate_group(A);
    w.announce(A, &n2).unwrap();
    // The clone holds the same DID key: its seq-3 note is indistinguishable from the owner's.
    let hostile = RotationNote::sign(&clone.id, g, "grp-clone-3", 3).wire();
    w.live.insert("grp-clone-3".into(), w.did(A));
    assert_eq!(w.inject(D, HOST, &hostile).map(|n| n.claims.seq), Ok(3));
    assert_eq!(w.known_endpoint(HOST, A), Some("grp-clone-3".into()));
    assert_eq!(w.reach(HOST, A), Ok(()));
    for i in [C, D] {
        assert_eq!(w.known_endpoint(i, A), None);
    }
    // Honest alice still believes she is at seq 2; her next rotation (seq 3) is a fork.
    let honest3 = w.rotate_group(A);
    let before = w.host_snapshot();
    assert_eq!(w.inject(A, HOST, &honest3), Err(Reject::ForkSameSeq));
    assert_eq!(w.host_snapshot(), before);
}
