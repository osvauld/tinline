//! W2-R: rotation notes relayed by the group host (user decisions U1, U2).
//!
//! Simulation only. A `RotationNote` is a stand-in for a succession permit: the owner DID signs
//! (owner, audience, endpoint, seq) with its real Ed25519 DID key. It is NOT the succession
//! verifier (G-D), carries no device path, and no future endpoint is derived from anything:
//! every endpoint is a fresh fixture string. Time is not modelled (no expiry/grace, G-TIME).
//! Membership is a fixed roster fixture; group authorization stays in `authority.rs`.
//! Endpoint strings: group endpoints start with `grp-`, private endpoints with `PRIVATE-`, so
//! audience leaks can be found by scanning bytes.
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

// Group-audience directory. The host holds one; every member holds one. Serializable so that
// rejection tests compare full bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct GroupView {
    group: String,
    roster: BTreeSet<String>,
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
    // Notes a sender offers on catch-up/forwarding, excluding the receiver's own path.
    fn export(&self, mode: CatchUp, skip: &str) -> Vec<Vec<u8>> {
        let mut out = vec![];
        for (owner, h) in &self.history {
            if owner == skip {
                continue;
            }
            match mode {
                CatchUp::FullHistory => out.extend(h.iter().map(RotationNote::wire)),
                CatchUp::LatestOnly => out.extend(h.last().map(RotationNote::wire)),
            }
        }
        out
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
    // Join bootstrap (invite/ticket) is out of scope: every view starts with every seq-1 note.
    for node in &mut w.nodes {
        for b in &bootstrap {
            node.view.accept(policy, b).unwrap();
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
    fn group_endpoint(&self, i: usize) -> String {
        self.nodes[i]
            .view
            .current(&self.nodes[i].did)
            .unwrap()
            .claims
            .endpoint
            .clone()
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
    // Host relays a note it accepted to every member it can reach (except the owner).
    fn fanout(&mut self, bytes: &[u8]) -> Vec<usize> {
        let owner: RotationNote = serde_json::from_slice(bytes).unwrap();
        let mut delivered = vec![];
        for j in 1..self.nodes.len() {
            if self.nodes[j].did == owner.claims.owner || self.reach(HOST, j).is_err() {
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
    // Member sends its own group note to the host, which relays it (U1, group audience).
    fn announce(&mut self, i: usize, bytes: &[u8]) -> Result<Vec<usize>, RouteErr> {
        if i == HOST {
            return Ok(self.fanout(bytes));
        }
        self.reach(i, HOST)?;
        self.send(i, HOST, bytes);
        let policy = self.policy;
        match self.nodes[HOST].view.accept(policy, bytes) {
            Ok(_) => Ok(self.fanout(bytes)),
            Err(_) => Ok(vec![]),
        }
    }
    // Member returns: re-announces its current note (its live side retries) and pulls what it
    // missed from the host. Returns number of newly accepted notes.
    fn catch_up(&mut self, i: usize) -> Result<usize, RouteErr> {
        self.reach(i, HOST)?;
        let own = self.nodes[i].view.current(&self.did(i)).unwrap().wire();
        self.announce(i, &own)?;
        let notes = self.nodes[HOST]
            .view
            .export(self.policy.catch_up, &self.did(i));
        let mut accepted = 0;
        for bytes in notes {
            self.send(HOST, i, &bytes);
            self.nodes[HOST].relayed.push(bytes.clone());
            let policy = self.policy;
            if self.nodes[i].view.accept(policy, &bytes).is_ok() {
                accepted += 1;
            }
        }
        Ok(accepted)
    }
    // Member-to-member forwarding of DID-signed notes (used when the host rotated).
    fn forward(&mut self, from: usize, to: usize) -> Result<usize, RouteErr> {
        self.reach(from, to)?;
        let notes = self.nodes[from]
            .view
            .export(self.policy.catch_up, &self.did(to));
        let mut accepted = 0;
        for bytes in notes {
            if self.inject(from, to, &bytes).is_ok() {
                accepted += 1;
            }
        }
        Ok(accepted)
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
fn member_rotation_is_relayed_to_online_members_and_offline_member_catches_up_from_host() {
    for policy in all_policies() {
        let mut w = world(policy);
        w.nodes[D].online = false;
        let old = w.group_endpoint(A);
        let note = w.rotate_group(A);
        let new = w.group_endpoint(A);
        assert_ne!(old, new);
        // Host relays the original signed bytes to the online member only.
        assert_eq!(w.announce(A, &note), Ok(vec![C]));
        assert_eq!(w.known_endpoint(HOST, A), Some(new.clone()));
        assert_eq!(w.known_endpoint(C, A), Some(new.clone()));
        assert_eq!(w.known_endpoint(D, A), Some(old.clone()));
        assert!(w.nodes[HOST].relayed.iter().all(|r| r == &note));
        assert_eq!(w.reach(C, A), Ok(()));
        // Offline member's stale route fails (old endpoint gone), no fallback.
        w.nodes[D].online = true;
        assert_eq!(w.reach(D, A), Err(RouteErr::EndpointGone));
        assert_eq!(w.catch_up(D), Ok(1));
        assert_eq!(w.known_endpoint(D, A), Some(new.clone()));
        assert_eq!(w.reach(D, A), Ok(()));
        // All four group views agree on every member's current group endpoint.
        for i in 1..4 {
            for t in 0..4 {
                assert_eq!(
                    w.known_endpoint(i, t),
                    w.known_endpoint(HOST, t),
                    "{policy:?}"
                );
            }
        }
    }
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
        w.forward(A, D).unwrap();
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
#[test]
fn two_members_rotated_while_apart_recover_each_other_via_host() {
    for policy in all_policies() {
        let mut w = world(policy);
        w.nodes[A].online = false;
        w.nodes[D].online = false;
        // Both rotate while offline: nobody learns either new endpoint yet.
        let _ = w.rotate_group(A);
        let _ = w.rotate_group(D);
        let (a_new, d_new) = (w.group_endpoint(A), w.group_endpoint(D));
        w.nodes[A].online = true;
        assert_eq!(w.catch_up(A), Ok(0)); // host has nothing new for alice yet
        assert_eq!(w.known_endpoint(A, D), Some("grp-dave-1".into()));
        assert_eq!(w.reach(A, D), Err(RouteErr::EndpointGone));
        w.nodes[D].online = true;
        // Dave's live side retries: re-announce relays dave's note to alice (online), and
        // the host returns alice's newer note to dave.
        assert_eq!(w.catch_up(D), Ok(1));
        assert_eq!(w.known_endpoint(A, D), Some(d_new.clone()));
        assert_eq!(w.known_endpoint(D, A), Some(a_new.clone()));
        assert_eq!(w.reach(A, D), Ok(()));
        assert_eq!(w.reach(D, A), Ok(()));
        assert_eq!(w.known_endpoint(C, A), Some(a_new));
        assert_eq!(w.known_endpoint(C, D), Some(d_new));
    }
}

// KIND: invariant
#[test]
fn host_rotation_reaches_offline_member_through_verified_member_forwarding() {
    for policy in all_policies() {
        let mut w = world(policy);
        w.nodes[C].online = false;
        let host_note = w.rotate_group(HOST);
        assert_eq!(w.announce(HOST, &host_note), Ok(vec![A, D]));
        let host_new = w.group_endpoint(HOST);
        w.nodes[C].online = true;
        // Carol's only known host endpoint is gone: she cannot catch up from the host.
        assert_eq!(w.catch_up(C), Err(RouteErr::EndpointGone));
        // Dave forwards tampered / mis-signed host notes: carol verifies and refuses them.
        let before = w.view_snapshot(C);
        let mut tampered: RotationNote = serde_json::from_slice(&host_note).unwrap();
        tampered.claims.endpoint = "grp-evil".into();
        assert_eq!(w.inject(D, C, &tampered.wire()), Err(Reject::BadSignature));
        let mut by_dave = RotationNote::sign(
            &w.nodes[D].author.id,
            NoteAudience::Group(w.group.clone()),
            "grp-evil",
            2,
        );
        by_dave.claims.owner = w.did(HOST);
        assert_eq!(w.inject(D, C, &by_dave.wire()), Err(Reject::BadSignature));
        assert_eq!(w.view_snapshot(C), before);
        // Alice (who got the host note directly) forwards the genuine signed note.
        assert!(w.forward(A, C).unwrap() >= 1);
        assert_eq!(w.known_endpoint(C, HOST), Some(host_new));
        // What carol holds is the host's original DID-signed bytes, not a forwarder's copy.
        let held = w.nodes[C].view.current(&w.did(HOST)).unwrap().wire();
        assert_eq!(held, host_note);
        assert_eq!(w.catch_up(C), Ok(0));
        assert_eq!(w.reach(C, HOST), Ok(()));
    }
}

// KIND: invariant
#[test]
fn forged_stale_replayed_wrong_owner_private_and_foreign_notes_leave_host_and_member_unchanged() {
    for policy in all_policies() {
        let mut w = world(policy);
        let n2 = w.rotate_group(A);
        w.announce(A, &n2).unwrap();
        let a_id = &w.nodes[A].author.id;
        let group = NoteAudience::Group(w.group.clone());
        let n1 = RotationNote::sign(a_id, group.clone(), "grp-alice-1", 1).wire();
        let mut forged: RotationNote = serde_json::from_slice(&n2).unwrap();
        forged.claims.seq = 3;
        forged.claims.endpoint = "grp-alice-hijack".into();
        let mut flipped: RotationNote = serde_json::from_slice(&n2).unwrap();
        flipped.signature[7] ^= 1;
        let mut short_sig: RotationNote = serde_json::from_slice(&n2).unwrap();
        short_sig.signature.truncate(63);
        let stale = RotationNote::sign(a_id, group.clone(), "grp-alice-rollback", 1).wire();
        let mut wrong_owner =
            RotationNote::sign(&w.nodes[C].author.id, group.clone(), "grp-alice-x", 3);
        wrong_owner.claims.owner = w.did(A);
        let outsider = Author::new(9, &w.group);
        let not_member = RotationNote::sign(&outsider.id, group.clone(), "grp-out-1", 1).wire();
        let private = RotationNote::sign(a_id, NoteAudience::Private, "PRIVATE-alice-9", 3).wire();
        let foreign =
            RotationNote::sign(a_id, NoteAudience::Group("other".into()), "grp-a-o", 3).wire();
        let cases: Vec<(&str, Vec<u8>, Reject)> = vec![
            ("forged", forged.wire(), Reject::BadSignature),
            ("bit-flip", flipped.wire(), Reject::BadSignature),
            ("short-sig", short_sig.wire(), Reject::BadSignature),
            ("stale", stale, Reject::Stale),
            ("replay-current", n2.clone(), Reject::Replay),
            ("replay-old", n1, Reject::Replay),
            ("wrong-owner", wrong_owner.wire(), Reject::BadSignature),
            ("not-member", not_member, Reject::NotMember),
            ("private", private, Reject::PrivateAudience),
            ("wrong-group", foreign, Reject::WrongGroup),
            ("malformed", b"{\"claims\":1}".to_vec(), Reject::Malformed),
        ];
        for (name, bytes, want) in cases {
            let (host_before, c_before) = (w.host_snapshot(), w.view_snapshot(C));
            assert_eq!(
                w.inject(D, HOST, &bytes),
                Err(want),
                "host {name} {policy:?}"
            );
            assert_eq!(w.inject(D, C, &bytes), Err(want), "member {name}");
            assert_eq!(w.host_snapshot(), host_before, "host mutated by {name}");
            assert_eq!(w.view_snapshot(C), c_before, "member mutated by {name}");
        }
        // Positive control on the same populated state.
        let n3 = w.rotate_group(A);
        assert_eq!(w.announce(A, &n3), Ok(vec![C, D]));
        assert_eq!(w.known_endpoint(C, A), Some("grp-alice-3".into()));
    }
}

// KIND: invariant
#[test]
fn host_cannot_substitute_endpoint_in_relayed_note() {
    for policy in all_policies() {
        let mut w = world(policy);
        let note = w.rotate_group(A);
        // Alice delivers to the host; the host then tries to relay a substituted endpoint.
        w.reach(A, HOST).unwrap();
        w.inject(A, HOST, &note).unwrap();
        let mut swapped: RotationNote = serde_json::from_slice(&note).unwrap();
        swapped.claims.endpoint = "grp-host-mitm".into();
        let before = w.view_snapshot(C);
        assert_eq!(
            w.inject(HOST, C, &swapped.wire()),
            Err(Reject::BadSignature)
        );
        assert_eq!(w.view_snapshot(C), before);
        assert_eq!(w.inject(HOST, C, &note).map(|n| n.claims.seq), Ok(2));
    }
}

// KIND: invariant
#[test]
fn personal_block_does_not_stop_group_notes_or_change_group_state() {
    for policy in all_policies() {
        let run = |block: bool| {
            let mut w = world(policy);
            if block {
                // Alice personally blocks carol (private audience only, U5).
                let c = w.did(C);
                w.nodes[A].blocked.insert(c);
            }
            let private_recipients = w.rotate_private(A);
            let n = w.rotate_group(A);
            let delivered = w.announce(A, &n).unwrap();
            let n = w.rotate_group(C);
            w.announce(C, &n).unwrap();
            let views: Vec<Vec<u8>> = (0..4).map(|i| w.view_snapshot(i)).collect();
            (w.host_snapshot(), views, delivered, private_recipients, w)
        };
        let (host_plain, views_plain, del_plain, priv_plain, _) = run(false);
        let (host_block, views_block, del_block, priv_block, w) = run(true);
        // Carol still receives alice's group note and alice still receives carol's.
        assert_eq!(del_block, vec![C, D]);
        assert_eq!(w.known_endpoint(C, A), Some("grp-alice-2".into()));
        assert_eq!(w.known_endpoint(A, C), Some("grp-carol-2".into()));
        assert_eq!(w.reach(C, A), Ok(()));
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

// KIND: limitation
#[test]
fn host_and_all_forwarders_unreachable_means_no_recovery_without_reshare() {
    let mut w = world(all_policies()[0]);
    w.nodes[C].online = false;
    for i in [HOST, A, D] {
        let n = w.rotate_group(i);
        w.announce(i, &n).unwrap();
    }
    // Carol also rotated while away: nobody holds her new endpoint and she holds nobody's.
    let _ = w.rotate_group(C);
    w.nodes[C].online = true;
    assert_eq!(w.catch_up(C), Err(RouteErr::EndpointGone));
    for i in [A, D] {
        assert_eq!(w.forward(C, i), Err(RouteErr::EndpointGone));
        assert_eq!(w.forward(i, C), Err(RouteErr::EndpointGone));
    }
    assert_eq!(w.reach(HOST, C), Err(RouteErr::EndpointGone));
    // The new endpoints are live, yet nothing in carol's view names them: "lost contact".
    for i in [HOST, A, D] {
        let ep = w.group_endpoint(i);
        assert!(w.live.contains_key(&ep));
        assert_ne!(w.known_endpoint(C, i), Some(ep));
    }
    // Manual re-share (ticket/QR, out of band) of alice's current note restores a path; it
    // is still verified, then normal forwarding repairs the rest.
    let reshared = w.nodes[A].view.current(&w.did(A)).unwrap().wire();
    let policy = w.policy;
    w.nodes[C].view.accept(policy, &reshared).unwrap();
    assert_eq!(w.reach(C, A), Ok(()));
    let carol_now = w.nodes[C].view.current(&w.did(C)).unwrap().wire();
    w.inject(C, A, &carol_now).unwrap();
    w.forward(A, C).unwrap();
    assert_eq!(w.catch_up(C), Ok(0));
    assert_eq!(w.reach(D, C), Ok(()));
}

// KIND: candidate-policy G-HISTORY
#[test]
fn catch_up_latest_only_vs_full_history_under_gap_policies() {
    // Carol misses three rotations of alice (seq 2..4).
    let mut results = vec![];
    for policy in all_policies()
        .into_iter()
        .filter(|p| p.fork == ForkPolicy::KeepFirst)
    {
        let mut w = world(policy);
        w.nodes[C].online = false;
        for _ in 0..3 {
            let n = w.rotate_group(A);
            w.announce(A, &n).unwrap();
        }
        w.nodes[C].online = true;
        let before = w.view_snapshot(C);
        let wire_before = w.wire.len();
        let accepted = w.catch_up(C).unwrap();
        let sent: Vec<&Frame> = w.wire[wire_before..]
            .iter()
            .filter(|f| f.from == w.did(HOST) && f.to == w.did(C))
            .collect();
        let bytes: usize = sent.iter().map(|f| f.bytes.len()).sum();
        let routed = w.reach(C, A);
        match (policy.catch_up, policy.gap) {
            (CatchUp::LatestOnly, GapPolicy::RequireContiguous) => {
                // Strict receiver cannot use a latest-only host: gap refused, view unchanged,
                // carol stays stuck on alice's retired endpoint.
                assert_eq!(accepted, 0);
                assert_eq!(w.view_snapshot(C), before);
                assert_eq!(routed, Err(RouteErr::EndpointGone));
            }
            (CatchUp::LatestOnly, GapPolicy::AcceptGap) => {
                assert_eq!((accepted, sent.len()), (1, 3));
                assert_eq!(routed, Ok(()));
            }
            (CatchUp::FullHistory, _) => {
                assert_eq!((accepted, sent.len()), (3, 6));
                assert_eq!(routed, Ok(()));
            }
        }
        results.push((policy.catch_up, policy.gap, sent.len(), bytes));
    }
    // Full history costs more bytes on every catch-up and grows with rotation count.
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
        let mut dave = w.nodes[D].view.clone();
        assert!(host.accept(policy, &honest).is_ok());
        assert_eq!(host.accept(policy, &forked), Err(Reject::ForkSameSeq));
        assert!(dave.accept(policy, &forked).is_ok());
        assert_eq!(dave.accept(policy, &honest), Err(Reject::ForkSameSeq));
        let a = w.did(A);
        match fork {
            ForkPolicy::KeepFirst => {
                // Arrival order picks the winner: host and dave route alice differently.
                assert_eq!(host.route(&a).unwrap().claims.endpoint, "grp-alice-2");
                assert_eq!(dave.route(&a).unwrap().claims.endpoint, "grp-alice-clone-2");
            }
            ForkPolicy::Freeze => {
                // Order-independent, but alice's group path is frozen everywhere: any
                // mnemonic holder can DoS it. Later honest notes are refused too.
                assert!(host.route(&a).is_none() && dave.route(&a).is_none());
                assert_eq!(host.frozen, dave.frozen);
                let n3 = RotationNote::sign(&w.nodes[A].author.id, g, "grp-alice-3", 3).wire();
                let before = serde_json::to_vec(&host).unwrap();
                assert_eq!(host.accept(policy, &n3), Err(Reject::Frozen));
                assert_eq!(serde_json::to_vec(&host).unwrap(), before);
            }
        }
    }
}

// KIND: limitation
#[test]
fn mnemonic_clone_with_higher_seq_replaces_honest_group_endpoint_everywhere() {
    let mut w = world(all_policies()[0]);
    let clone = Author::new(1, &w.group);
    let g = NoteAudience::Group(w.group.clone());
    let n2 = w.rotate_group(A);
    w.announce(A, &n2).unwrap();
    // The clone holds the same DID key: its seq-3 note is indistinguishable from the owner's.
    let hostile = RotationNote::sign(&clone.id, g, "grp-clone-3", 3).wire();
    w.live.insert("grp-clone-3".into(), w.did(A));
    assert_eq!(w.inject(D, HOST, &hostile).map(|n| n.claims.seq), Ok(3));
    assert_eq!(w.fanout(&hostile), vec![C, D]);
    for i in [HOST, C, D] {
        assert_eq!(w.known_endpoint(i, A), Some("grp-clone-3".into()));
    }
    // Honest alice still believes she is at seq 2; her next rotation (seq 3) now looks
    // like a fork, and nothing here can tell which device is the owner's.
    let honest3 = w.rotate_group(A);
    let before = w.host_snapshot();
    assert_eq!(w.inject(A, HOST, &honest3), Err(Reject::ForkSameSeq));
    assert_eq!(w.host_snapshot(), before);
}
