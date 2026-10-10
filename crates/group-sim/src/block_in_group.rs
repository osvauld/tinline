//! W2-G: a personal DID block inside a group (user decisions U5, U6). Status: proposal — not
//! approved. Simulation only; no production API, wire format or UI.
//!
//! Model (test-only):
//! - A personal block is a LOCAL set of DIDs owned by one member (`BTreeSet<String>`). It is
//!   never signed, never sent, and is not a group op. Shared group state is the existing
//!   `Replica` (signed genesis + delegated `Grant`s, real Loro, real Ed25519) — unchanged code.
//! - "Host" = a `Replica` that members upload to and are served from (as in `tests.rs`). A
//!   `HostRelay` parameter encodes the OPEN question whether a host that blocked someone may
//!   withhold relay (G-BLOCK); `Neutral` is the only option that leaves group state untouched.
//! - Local display of a blocked member's group messages is a pure function of (replica, block
//!   list, `Display`); the three `Display` options are candidates, none is selected.
//! - Endpoint notes are a sim stand-in for permits: DID-signed (owner, audience, endpoint, seq).
//!   The private note goes directly to unblocked contacts (U1/U5); the group note goes through
//!   the host (U1/U2). No succession verifier is built here (G-D).
//! - Chat requests from a blocked member (U3) are W2-Q's scope and are not modelled here.
use super::*;
use identity::{public_key_from_did, verify};

const NOTE_DOMAIN: &[u8] = b"tinline/group-sim/endpoint-note/not-production/v1\0";

// ---- fixtures ------------------------------------------------------------------------------

struct World {
    g: Genesis,
    a: Author, // creator (root grant to self)
    b: Author, // admin, issued C's grant
    c: Author, // member (the one who gets blocked)
    d: Author, // member, grant issued directly by the creator
}
fn world() -> World {
    let (g, a, b, c) = signed_fixture();
    let mut d = Author::new(4, "unconfigured");
    let grant = Grant::issue(
        &a.id,
        d.id.did(),
        &g.id(),
        &[Capability::Read, Capability::Post],
        false,
        4,
        None,
    );
    d.configure(&g.id(), grant);
    World { g, a, b, c, d }
}
fn replica(g: &Genesis) -> Replica {
    Replica::from_genesis(g.clone(), &g.id()).unwrap()
}
fn did(a: &Author) -> String {
    a.id.did().to_string()
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn endpoint(key: &SigningKey) -> String {
    hex(&key.verifying_key().to_bytes())
}
// What a receiver gets on the wire for one envelope: signed bytes, signature, attestation.
fn envelope_wire(e: &Envelope) -> Vec<u8> {
    [
        e.bytes(),
        e.signature.to_vec(),
        serde_json::to_vec(&e.attestation).unwrap(),
    ]
    .concat()
}
// Fixture: author's device imports what its replica accepted (causal order, skip own posts).
fn catch_up(author: &mut Author, r: &Replica) {
    for e in causal_order(r.accepted.values().cloned().collect()) {
        if e.body.author != author.id.did() && !author.observed.contains(&e.id()) {
            author.observe(&e);
        }
    }
}
fn ids(r: &Replica) -> BTreeSet<String> {
    r.accepted.keys().cloned().collect()
}

// ---- host relay candidates (G-BLOCK, open) -------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum HostRelay {
    /// Host relays every valid envelope regardless of its operator's personal block list.
    Neutral,
    /// Host still accepts and stores blocked authors' posts but does not forward them.
    WithholdBlockedAuthors,
    /// Host neither accepts uploads from nor serves a member its operator blocked.
    RefuseBlockedMember,
}
fn upload(
    host: &mut Replica,
    blocks: &BTreeSet<String>,
    relay: HostRelay,
    from: &str,
    e: &Envelope,
) {
    if relay == HostRelay::RefuseBlockedMember && blocks.contains(from) {
        return; // silently dropped
    }
    host.receive(e.clone());
}
fn serve(
    host: &Replica,
    blocks: &BTreeSet<String>,
    relay: HostRelay,
    to: &str,
    r: &mut Replica,
    wire: &mut Vec<Vec<u8>>,
) {
    if relay == HostRelay::RefuseBlockedMember && blocks.contains(to) {
        return;
    }
    for e in causal_order(host.accepted.values().cloned().collect()) {
        if r.accepted.contains_key(&e.id()) {
            continue; // as if a VV exchange had completed
        }
        if relay == HostRelay::WithholdBlockedAuthors && blocks.contains(&e.body.author) {
            continue;
        }
        wire.push(envelope_wire(&e));
        r.receive(e);
    }
}

// One deterministic trace. B is the member who may block C. If `host_is_blocker`, B's replica
// IS the host replica (B hosts the group); otherwise a separate host H relays and B is a member.
// D and C are also direct contacts and sync with each other once (step 4).
struct Run {
    g: Genesis,
    replicas: BTreeMap<&'static str, Replica>,
    wire: BTreeMap<&'static str, Vec<Vec<u8>>>,
    grants: Vec<u8>,
    post: BTreeMap<&'static str, String>,
    b_blocks: BTreeSet<String>,
    c_did: String,
}
#[derive(Debug, PartialEq)]
struct Fingerprint {
    snapshots: BTreeMap<&'static str, Snapshot>,
    wire: BTreeMap<&'static str, Vec<Vec<u8>>>,
    grants: Vec<u8>,
    post: BTreeMap<&'static str, String>,
}
impl Run {
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            snapshots: self
                .replicas
                .iter()
                .map(|(k, r)| (*k, r.snapshot()))
                .collect(),
            wire: self.wire.clone(),
            grants: self.grants.clone(),
            post: self.post.clone(),
        }
    }
    fn id(&self, name: &str) -> &String {
        &self.post[name]
    }
}
fn run(host_is_blocker: bool, block: bool, relay: HostRelay) -> Run {
    let World {
        g,
        mut a,
        mut b,
        mut c,
        mut d,
    } = world();
    let (da, db, dc, dd) = (did(&a), did(&b), did(&c), did(&d));
    // B's personal, local block list. Nothing in the shared path reads it unless B hosts.
    let mut b_blocks = BTreeSet::new();
    if block {
        b_blocks.insert(dc.clone());
    }
    let host_blocks = if host_is_blocker {
        b_blocks.clone()
    } else {
        BTreeSet::new()
    };
    let mut host = replica(&g);
    let (mut ra, mut rb, mut rc, mut rd) = (replica(&g), replica(&g), replica(&g), replica(&g));
    let mut wire: BTreeMap<&'static str, Vec<Vec<u8>>> = BTreeMap::new();
    let mut post = BTreeMap::new();

    // 1-2: A and C post and upload.
    let a1 = a.post("a1");
    ra.receive(a1.clone());
    upload(&mut host, &host_blocks, relay, &da, &a1);
    let c1 = c.post("c1");
    rc.receive(c1.clone());
    upload(&mut host, &host_blocks, relay, &dc, &c1);
    // 3: host serves members.
    let mut serve_all =
        |host: &Replica, ra: &mut Replica, rb: &mut Replica, rc: &mut Replica, rd: &mut Replica| {
            serve(
                host,
                &host_blocks,
                relay,
                &da,
                ra,
                wire.entry("a").or_default(),
            );
            if !host_is_blocker {
                serve(
                    host,
                    &host_blocks,
                    relay,
                    &db,
                    rb,
                    wire.entry("b").or_default(),
                );
            }
            serve(
                host,
                &host_blocks,
                relay,
                &dc,
                rc,
                wire.entry("c").or_default(),
            );
            serve(
                host,
                &host_blocks,
                relay,
                &dd,
                rd,
                wire.entry("d").or_default(),
            );
        };
    serve_all(&host, &mut ra, &mut rb, &mut rc, &mut rd);
    // 4: D syncs directly with its contact C.
    rd.sync_from(&rc);
    // 5: B replies after importing what its replica holds (hidden or not, content is held).
    catch_up(&mut b, if host_is_blocker { &host } else { &rb });
    let b1 = b.post("b1");
    if host_is_blocker {
        host.receive(b1.clone());
    } else {
        rb.receive(b1.clone());
        upload(&mut host, &host_blocks, relay, &db, &b1);
    }
    // 6: D replies to C's post.
    catch_up(&mut d, &rd);
    let d1 = d.post("d1 re c1");
    assert!(d1.body.dependencies.contains(&c1.id()));
    rd.receive(d1.clone());
    upload(&mut host, &host_blocks, relay, &dd, &d1);
    // 7: C posts again.
    let c2 = c.post("c2");
    rc.receive(c2.clone());
    upload(&mut host, &host_blocks, relay, &dc, &c2);
    // 8: final serve.
    serve_all(&host, &mut ra, &mut rb, &mut rc, &mut rd);

    for (k, e) in [
        ("a1", &a1),
        ("c1", &c1),
        ("b1", &b1),
        ("d1", &d1),
        ("c2", &c2),
    ] {
        post.insert(k, e.id());
    }
    let grants = serde_json::to_vec(&[&a.permit, &b.permit, &c.permit, &d.permit]).unwrap();
    let mut replicas = BTreeMap::from([("host", host), ("a", ra), ("c", rc), ("d", rd)]);
    if !host_is_blocker {
        replicas.insert("b", rb);
    }
    Run {
        g,
        replicas,
        wire,
        grants,
        post,
        b_blocks,
        c_did: dc,
    }
}

// ---- local display candidates (G-BLOCK, open) ----------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Display {
    Shown,
    Hidden,
    Collapsed,
}
#[derive(Debug, PartialEq)]
enum Line {
    Message { author: String, text: String },
    // Placeholder for a run of consecutive blocked messages; carries no author text.
    Collapsed { count: usize },
}
// Pure view function: reads the replica, never mutates it.
fn render(r: &Replica, blocks: &BTreeSet<String>, display: Display) -> Vec<Line> {
    let mut out = vec![];
    for e in causal_order(r.accepted.values().cloned().collect()) {
        let blocked = blocks.contains(&e.body.author);
        match (blocked, display) {
            (false, _) | (true, Display::Shown) => out.push(Line::Message {
                author: e.body.author.clone(),
                text: e.body.text.clone(),
            }),
            (true, Display::Hidden) => {}
            (true, Display::Collapsed) => match out.last_mut() {
                Some(Line::Collapsed { count }) => *count += 1,
                _ => out.push(Line::Collapsed { count: 1 }),
            },
        }
    }
    out
}
fn texts(lines: &[Line]) -> Vec<&str> {
    lines
        .iter()
        .filter_map(|l| match l {
            Line::Message { text, .. } => Some(text.as_str()),
            Line::Collapsed { .. } => None,
        })
        .collect()
}

// ---- endpoint notes (sim stand-in for permits) ---------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum NoteAudience {
    Group(String),
    Private,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct NoteBody {
    owner: String,
    audience: NoteAudience,
    endpoint: String,
    seq: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Note {
    body: NoteBody,
    signature: Vec<u8>,
}
impl Note {
    fn bytes(body: &NoteBody) -> Vec<u8> {
        [NOTE_DOMAIN, &serde_json::to_vec(body).unwrap()].concat()
    }
    fn issue(owner: &Identity, audience: NoteAudience, endpoint: String, seq: u64) -> Self {
        let body = NoteBody {
            owner: owner.did().into(),
            audience,
            endpoint,
            seq,
        };
        let signature = owner.sign(&Self::bytes(&body)).to_vec();
        Self { body, signature }
    }
    fn verify(&self) -> bool {
        let Some(key) = public_key_from_did(&self.body.owner) else {
            return false;
        };
        let Ok(sig) = <&[u8; 64]>::try_from(self.signature.as_slice()) else {
            return false;
        };
        verify(&key, &Self::bytes(&self.body), sig)
    }
    fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}
// U5: the private note goes to accepted contacts minus the owner's block list.
fn private_recipients(contacts: &BTreeSet<String>, blocks: &BTreeSet<String>) -> Vec<String> {
    contacts.difference(blocks).cloned().collect()
}
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}
// Encodings of a 32-byte key that might appear in serialized data the receiver holds.
fn encodings(key: &[u8; 32]) -> Vec<Vec<u8>> {
    vec![
        key.to_vec(),
        hex(key).into_bytes(),
        serde_json::to_vec(key).unwrap(),
    ]
}
// Everything a member can extract: raw received bytes + replica export, and every key/endpoint
// it can decode (envelope device keys, attested device keys, verified note endpoints).
struct Learned {
    raw: Vec<u8>,
    endpoints: BTreeSet<String>,
    notes: Vec<Note>,
}
fn learned(inbox: &[Vec<u8>], r: &Replica) -> Learned {
    let mut raw = inbox.concat();
    raw.extend(r.doc.export(ExportMode::all_updates()).unwrap());
    let mut endpoints = BTreeSet::new();
    let mut notes = vec![];
    for e in r.accepted.values() {
        raw.extend(envelope_wire(e));
        endpoints.insert(hex(&e.body.device));
        let a = proto::verify_attestation(&e.attestation).unwrap();
        endpoints.insert(hex(&a.device_key().unwrap()));
    }
    for bytes in inbox {
        if let Ok(n) = serde_json::from_slice::<Note>(bytes) {
            if n.verify() {
                endpoints.insert(n.body.endpoint.clone());
                notes.push(n);
            }
        }
    }
    Learned {
        raw,
        endpoints,
        notes,
    }
}
fn leaks(l: &Learned, key: &[u8; 32]) -> bool {
    l.endpoints.contains(&hex(key)) || encodings(key).iter().any(|n| contains(&l.raw, n))
}

// Group scenario for the private-endpoint tests. B's author key, group transport key and
// private transport key(s) are given; B blocks C. Separate host H (neutral) relays.
struct EndpointScenario {
    c: Learned,
    a: Learned,
    c_replica_ids: BTreeSet<String>,
    host_ids: BTreeSet<String>,
}
fn endpoint_scenario(group_key: &SigningKey, private_keys: &[SigningKey]) -> EndpointScenario {
    let World {
        g,
        mut a,
        mut b,
        mut c,
        d,
    } = world();
    let gid = g.id();
    let contacts: BTreeSet<String> = [did(&a), did(&c), did(&d)].into();
    let blocks: BTreeSet<String> = [did(&c)].into();
    let mut inbox: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
    // Private notes (initial + periodic rotations): direct, unblocked contacts only.
    for (i, key) in private_keys.iter().enumerate() {
        let n = Note::issue(&b.id, NoteAudience::Private, endpoint(key), i as u64 + 1);
        for to in private_recipients(&contacts, &blocks) {
            inbox.entry(to).or_default().push(n.wire());
        }
    }
    // Group note: to the host, which relays it to every member (C included: a personal block
    // does not change group membership or the group audience).
    let gn = Note::issue(&b.id, NoteAudience::Group(gid), endpoint(group_key), 1);
    // Group content from B, A and C.
    let mut host = replica(&g);
    host.receive(a.post("hi from a"));
    host.receive(c.post("hi from c"));
    catch_up(&mut b, &host);
    host.receive(b.post("hi from b"));
    let (mut ra, mut rc) = (replica(&g), replica(&g));
    for (to, r) in [(did(&a), &mut ra), (did(&c), &mut rc)] {
        let w = inbox.entry(to.clone()).or_default();
        serve(&host, &BTreeSet::new(), HostRelay::Neutral, &to, r, w);
        w.push(gn.wire());
    }
    EndpointScenario {
        c: learned(&inbox[&did(&c)], &rc),
        a: learned(&inbox[&did(&a)], &ra),
        c_replica_ids: ids(&rc),
        host_ids: ids(&host),
    }
}

// ---- tests ---------------------------------------------------------------------------------

// KIND: invariant
// Personal block (B blocks C) leaves every replica (host, A, B, C, D), every relayed byte and
// every grant byte-identical to the run without the block, whether B is a member or the host,
// as long as the host relays neutrally. The block list lives outside shared state.
#[test]
fn personal_block_leaves_every_replica_grant_and_relayed_byte_unchanged() {
    for host_is_blocker in [false, true] {
        let plain = run(host_is_blocker, false, HostRelay::Neutral);
        // Determinism control: the comparison below is not vacuous noise.
        assert_eq!(
            run(host_is_blocker, false, HostRelay::Neutral).fingerprint(),
            plain.fingerprint()
        );
        let blocked = run(host_is_blocker, true, HostRelay::Neutral);
        assert!(blocked.b_blocks.contains(&blocked.c_did));
        assert_eq!(blocked.fingerprint(), plain.fingerprint());
        // Convergence: every replica holds all five posts, nothing pending.
        let host = &blocked.replicas["host"];
        assert_eq!(host.accepted.len(), 5);
        for r in blocked.replicas.values() {
            host.assert_same(r);
        }
        // C's grant still authorizes posting in the blocked run.
        let c_post = &blocked.replicas["host"].accepted[blocked.id("c2")];
        assert!(c_post.body.permit.as_ref().unwrap().allows(
            &blocked.g,
            &blocked.c_did,
            Capability::Post
        ));
    }
    // Sensitivity control: if the block DID reach the shared path (a withholding host), the
    // fingerprint comparison detects it.
    assert_ne!(
        run(true, true, HostRelay::WithholdBlockedAuthors).fingerprint(),
        run(true, false, HostRelay::WithholdBlockedAuthors).fingerprint()
    );
}

// KIND: candidate-policy G-BLOCK
// Display option (a) shown: the blocker sees the blocked member's group messages as usual.
#[test]
fn display_candidate_shown_renders_blocked_members_messages_unchanged() {
    let r = run(false, true, HostRelay::Neutral);
    let rb = &r.replicas["b"];
    let before = rb.snapshot();
    let lines = render(rb, &r.b_blocks, Display::Shown);
    assert_eq!(rb.snapshot(), before, "render mutated replica");
    assert_eq!(lines, render(rb, &BTreeSet::new(), Display::Shown));
    let t = texts(&lines);
    assert!(t.contains(&"c1") && t.contains(&"c2") && t.contains(&"d1 re c1"));
    assert_eq!(t.len(), 5);
}

// KIND: candidate-policy G-BLOCK
// Display option (b) hidden: blocked member's messages are not rendered, but they remain in
// the blocker's replica, converge, are relayed onward by the blocker, and a reply that depends
// on them still renders. Unblock restores them immediately with no network round trip.
#[test]
fn display_candidate_hidden_keeps_content_converged_and_relayed() {
    let r = run(false, true, HostRelay::Neutral);
    let rb = &r.replicas["b"];
    let before = rb.snapshot();
    let lines = render(rb, &r.b_blocks, Display::Hidden);
    assert_eq!(rb.snapshot(), before, "render mutated replica");
    let t = texts(&lines);
    assert!(!t.contains(&"c1") && !t.contains(&"c2"));
    assert!(t.contains(&"d1 re c1"), "dependent reply must still render");
    assert!(
        lines
            .iter()
            .all(|l| matches!(l, Line::Message { author, .. } if *author != r.c_did))
    );
    // Still held and converged with the host.
    assert!(rb.accepted.contains_key(r.id("c1")) && rb.accepted.contains_key(r.id("c2")));
    r.replicas["host"].assert_same(rb);
    // Blocker's replica is not a censoring relay: a fresh member syncing only from B gets C.
    let mut e = replica(&r.g);
    e.sync_from(rb);
    rb.assert_same(&e);
    assert!(e.accepted.contains_key(r.id("c1")));
    // Unblock: purely local, immediate.
    assert_eq!(
        render(rb, &BTreeSet::new(), Display::Hidden),
        render(rb, &r.b_blocks, Display::Shown)
    );
    assert_eq!(rb.snapshot(), before);
}

// KIND: candidate-policy G-BLOCK
// Display option (c) collapsed placeholder: blocked messages become placeholders carrying a
// count only (no text); shared state unchanged; unblock restores the full view.
#[test]
fn display_candidate_collapsed_shows_placeholders_without_text() {
    let r = run(false, true, HostRelay::Neutral);
    let rb = &r.replicas["b"];
    let before = rb.snapshot();
    let lines = render(rb, &r.b_blocks, Display::Collapsed);
    assert_eq!(rb.snapshot(), before, "render mutated replica");
    let hidden: usize = lines
        .iter()
        .map(|l| match l {
            Line::Collapsed { count } => *count,
            _ => 0,
        })
        .sum();
    assert_eq!(hidden, 2);
    let t = texts(&lines);
    assert!(!t.contains(&"c1") && !t.contains(&"c2"));
    assert_eq!(t.len(), 3);
    assert_eq!(
        render(rb, &BTreeSet::new(), Display::Collapsed),
        render(rb, &r.b_blocks, Display::Shown)
    );
}

// KIND: invariant
// Hiding must stay a view-layer decision. Counter-model: a blocker that drops the blocked
// member's envelopes before its replica strands D's reply (and B's own reply) as pending and
// never converges with the host; the view-layer replica accepts them.
#[test]
fn replica_level_block_filter_strands_other_members_replies() {
    let r = run(false, true, HostRelay::Neutral);
    let host = &r.replicas["host"];
    let mut filtering = replica(&r.g);
    for e in causal_order(host.accepted.values().cloned().collect()) {
        if r.b_blocks.contains(&e.body.author) {
            continue; // the rejected design
        }
        filtering.receive(e);
    }
    assert!(filtering.pending.contains_key(r.id("d1")));
    assert!(filtering.pending.contains_key(r.id("b1")));
    assert!(!filtering.accepted.contains_key(r.id("d1")));
    assert_eq!(filtering.accepted.len(), 1); // only a1
    assert_ne!(ids(&filtering), ids(host));
    // View-layer block (same block list) keeps everything and renders D's reply.
    let rb = &r.replicas["b"];
    host.assert_same(rb);
    assert!(texts(&render(rb, &r.b_blocks, Display::Hidden)).contains(&"d1 re c1"));
}

// KIND: candidate-policy G-BLOCK
// Host-relay option "neutral": B hosts the group and blocks C, yet relays C's posts like any
// other. Everyone converges, and C's observation is identical to the unblocked run (silent).
#[test]
fn blocker_host_candidate_neutral_relay_converges_and_is_silent() {
    let blocked = run(true, true, HostRelay::Neutral);
    let plain = run(true, false, HostRelay::Neutral);
    let host = &blocked.replicas["host"];
    for r in blocked.replicas.values() {
        host.assert_same(r);
    }
    assert!(
        blocked.replicas["a"]
            .accepted
            .contains_key(blocked.id("c2"))
    );
    assert_eq!(blocked.wire["c"], plain.wire["c"]);
    assert_eq!(
        blocked.replicas["c"].snapshot(),
        plain.replicas["c"].snapshot()
    );
}

// KIND: candidate-policy G-BLOCK
// Host-relay option "withhold blocked authors": host B stores C's posts but does not forward
// them. Consequences measured: A never gets c1/c2 and B's OWN reply b1 and D's reply d1 are
// stranded pending at A (both causally follow c1); D lacks c2; C cannot tell (its state and
// wire equal the neutral run). C's grant is untouched. Repair needs a path around the host.
#[test]
fn blocker_host_candidate_withhold_strands_replies_and_is_invisible_to_blocked() {
    let w = run(true, true, HostRelay::WithholdBlockedAuthors);
    let neutral = run(true, true, HostRelay::Neutral);
    let host = &w.replicas["host"];
    assert_eq!(host.accepted.len(), 5);
    assert!(host.pending.is_empty());
    let ra = &w.replicas["a"];
    assert_eq!(ids(ra), BTreeSet::from([w.id("a1").clone()]));
    assert!(ra.pending.contains_key(w.id("b1")) && ra.pending.contains_key(w.id("d1")));
    let rd = &w.replicas["d"];
    assert!(rd.accepted.contains_key(w.id("c1")) && !rd.accepted.contains_key(w.id("c2")));
    // Invisible to C in this sim (no receipts/acks modelled).
    assert_eq!(w.wire["c"], neutral.wire["c"]);
    assert_eq!(w.replicas["c"].snapshot(), neutral.replicas["c"].snapshot());
    assert_eq!(w.grants, neutral.grants);
    // Repair only via other members: A from D (gets c1, drains b1/d1), then from C (c2).
    let mut ra = replica(&w.g);
    ra.sync_from(&w.replicas["a"]);
    ra.sync_from(rd);
    assert!(ra.accepted.contains_key(w.id("d1")) && !ra.accepted.contains_key(w.id("c2")));
    ra.sync_from(&w.replicas["c"]);
    host.assert_same(&ra);
}

// KIND: candidate-policy G-BLOCK
// Host-relay option "refuse blocked member": host B neither accepts from nor serves C. C keeps
// a valid grant but is de facto excluded from that host (an implicit membership effect), C can
// observe the difference (gets nothing), and D's reply to c1 strands pending at the host, so A
// never sees it either.
#[test]
fn blocker_host_candidate_refuse_member_is_de_facto_exclusion_and_observable() {
    let x = run(true, true, HostRelay::RefuseBlockedMember);
    let neutral = run(true, true, HostRelay::Neutral);
    let host = &x.replicas["host"];
    assert!(!host.accepted.contains_key(x.id("c1")) && !host.accepted.contains_key(x.id("c2")));
    assert!(host.pending.contains_key(x.id("d1")));
    assert!(!x.replicas["a"].accepted.contains_key(x.id("d1")));
    // C got nothing from the host: observable, unlike the neutral run.
    assert!(x.wire["c"].is_empty());
    assert_eq!(
        ids(&x.replicas["c"]),
        BTreeSet::from([x.id("c1").clone(), x.id("c2").clone()])
    );
    assert_ne!(x.replicas["c"].snapshot(), neutral.replicas["c"].snapshot());
    // ...while C's membership/grant is unchanged and its posts are valid elsewhere.
    assert_eq!(x.grants, neutral.grants);
    assert!(x.replicas["d"].accepted.contains_key(x.id("c1")));
    let c1 = &x.replicas["d"].accepted[x.id("c1")];
    assert!(
        c1.body
            .permit
            .as_ref()
            .unwrap()
            .allows(&x.g, &x.c_did, Capability::Post)
    );
}

// KIND: invariant
// With separate keys (U2 private/group transport keys, U4 long-lived author key), the blocked
// member C never learns B's private endpoint — neither the current one nor a rotated one —
// from anything it receives in the group (envelopes, attestations, relayed notes, replica
// export). Positive controls: C does learn B's group endpoint and author key; A (unblocked
// contact) gets both private notes, verified.
#[test]
fn blocked_member_never_learns_blockers_private_endpoint_through_group() {
    let group_key = SigningKey::from_bytes(&[0xB1; 32]);
    let private = [
        SigningKey::from_bytes(&[0xB2; 32]),
        SigningKey::from_bytes(&[0xB3; 32]),
    ];
    let s = endpoint_scenario(&group_key, &private);
    for k in &private {
        assert!(
            !leaks(&s.c, &k.verifying_key().to_bytes()),
            "private endpoint leaked to C"
        );
    }
    assert!(
        s.c.notes
            .iter()
            .all(|n| n.body.audience != NoteAudience::Private)
    );
    // Scanner positive controls.
    assert!(leaks(&s.c, &group_key.verifying_key().to_bytes()));
    let b_author = SigningKey::from_bytes(&[2; 32]).verifying_key().to_bytes();
    assert!(leaks(&s.c, &b_author));
    assert_eq!(
        s.c_replica_ids, s.host_ids,
        "C is still fully served by the group"
    );
    // Unblocked contact A got both private notes, verified, latest seq = 2.
    let a_private: Vec<&Note> =
        s.a.notes
            .iter()
            .filter(|n| n.body.audience == NoteAudience::Private)
            .collect();
    assert_eq!(a_private.len(), 2);
    assert_eq!(a_private[1].body.endpoint, endpoint(&private[1]));
    assert!(leaks(&s.a, &private[1].verifying_key().to_bytes()));
    // Real signature: a tampered note does not verify.
    let mut t = a_private[1].clone();
    t.body.endpoint = endpoint(&group_key);
    assert!(!t.verify());
}

// KIND: limitation
// Today's single-key model (D1 trace R1/R4: the device key is both the iroh EndpointId and
// the content-signing key). If the author key equals the private transport key, every group
// post carries it (envelope `device`, attestation), so the blocked member learns the private
// endpoint from group traffic even though it was excluded from every private note.
#[test]
fn author_key_equal_to_private_transport_key_leaks_private_endpoint_to_blocked_member() {
    let group_key = SigningKey::from_bytes(&[0xB1; 32]);
    let author_key = SigningKey::from_bytes(&[2; 32]); // B's author key in the fixture
    let s = endpoint_scenario(&group_key, &[author_key.clone()]);
    assert!(
        s.c.notes
            .iter()
            .all(|n| n.body.audience != NoteAudience::Private)
    );
    assert!(s.c.endpoints.contains(&endpoint(&author_key)));
    assert!(leaks(&s.c, &author_key.verifying_key().to_bytes()));
}

// KIND: invariant
// Blocks by the creator (A) and by the admin who issued C's grant (B) do not revoke or alter
// any grant; C's later posts are accepted by every replica including both blockers'; B's
// delegation authority is unaffected (it can still admit a new member E); grant bytes,
// genesis and the shared state of non-blockers are unchanged by the block.
#[test]
fn creator_and_admin_blocks_do_not_revoke_or_alter_grants() {
    let World {
        g,
        mut a,
        mut b,
        mut c,
        d,
    } = world();
    let grants = |a: &Author, b: &Author, c: &Author, d: &Author| {
        serde_json::to_vec(&(&g, [&a.permit, &b.permit, &c.permit, &d.permit])).unwrap()
    };
    let (mut host, mut ra, mut rb, mut rd) = (replica(&g), replica(&g), replica(&g), replica(&g));
    let c0 = c.post("c before block");
    for r in [&mut host, &mut ra, &mut rb, &mut rd] {
        assert_eq!(r.receive(c0.clone()), Outcome::Accepted);
    }
    let before = grants(&a, &b, &c, &d);
    let d_before = rd.snapshot();
    // The blocks: local sets only. No signed op is produced.
    let a_blocks: BTreeSet<String> = [did(&c)].into();
    let b_blocks: BTreeSet<String> = [did(&c)].into();
    assert_eq!(grants(&a, &b, &c, &d), before);
    assert_eq!(rd.snapshot(), d_before);
    let permit = c.permit.clone().unwrap();
    assert!(permit.allows(&g, c.id.did(), Capability::Post));
    assert_eq!(permit.body.issuer, did(&b)); // issued by the blocker, still valid
    // C keeps posting; every replica (blockers' included) accepts.
    let c1 = c.post("c after block");
    for r in [&mut host, &mut ra, &mut rb, &mut rd] {
        assert_eq!(r.receive(c1.clone()), Outcome::Accepted);
    }
    // Blockers' views can hide C, but their replicas hold C's posts.
    assert!(texts(&render(&ra, &a_blocks, Display::Hidden)).is_empty());
    assert_eq!(ra.accepted.len(), 2);
    // B's admin authority is intact: B admits E, whose post every replica accepts.
    let mut e = Author::new(5, "unconfigured");
    let grant_e = Grant::issue(
        &b.id,
        e.id.did(),
        &g.id(),
        &[Capability::Read, Capability::Post],
        false,
        5,
        b.permit.clone(),
    );
    e.configure(&g.id(), grant_e);
    let e1 = e.post("e joined");
    for r in [&mut host, &mut ra, &mut rb, &mut rd] {
        assert_eq!(r.receive(e1.clone()), Outcome::Accepted);
    }
    // A and B can still post too; the block never touched their grants.
    catch_up(&mut a, &host);
    assert_eq!(host.receive(a.post("a still posts")), Outcome::Accepted);
    catch_up(&mut b, &host);
    assert_eq!(host.receive(b.post("b still posts")), Outcome::Accepted);
    assert_eq!(grants(&a, &b, &c, &d), before);
    assert_eq!(
        texts(&render(&rb, &b_blocks, Display::Hidden)),
        vec!["e joined"]
    );
}
