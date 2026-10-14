use super::*;

// ---- fixtures ------------------------------------------------------------------------------

pub(super) struct World {
    pub(super) g: Genesis,
    pub(super) a: Author, // creator (root grant to self)
    pub(super) b: Author, // admin, issued C's grant
    pub(super) c: Author, // member (the one who gets blocked)
    pub(super) d: Author, // member, grant issued directly by the creator
}
pub(super) fn world() -> World {
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
pub(super) fn replica(g: &Genesis) -> Replica {
    Replica::from_genesis(g.clone(), &g.id()).unwrap()
}
pub(super) fn did(a: &Author) -> String {
    a.id.did().to_string()
}
// What a receiver gets on the wire for one envelope: signed bytes, signature, attestation.
pub(super) fn envelope_wire(e: &Envelope) -> Vec<u8> {
    [
        e.bytes(),
        e.signature.to_vec(),
        serde_json::to_vec(&e.attestation).unwrap(),
    ]
    .concat()
}
// Fixture: author's device imports what its replica accepted (causal order, skip own posts).
pub(super) fn catch_up(author: &mut Author, r: &Replica) {
    for e in causal_order(r.accepted.values().cloned().collect()) {
        if e.body.author != author.id.did() && !author.observed.contains(&e.id()) {
            author.observe(&e);
        }
    }
}
pub(super) fn ids(r: &Replica) -> BTreeSet<String> {
    r.accepted.keys().cloned().collect()
}

// ---- host relay candidates (G-BLOCK, open) -------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum HostRelay {
    /// Host relays every valid envelope regardless of its operator's personal block list.
    Neutral,
    /// Host still accepts and stores blocked authors' posts but does not forward them.
    WithholdBlockedAuthors,
    /// Host neither accepts uploads from nor serves a member its operator blocked.
    RefuseBlockedMember,
}
pub(super) fn upload(
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
pub(super) fn serve(
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
pub(super) struct Run {
    pub(super) g: Genesis,
    pub(super) replicas: BTreeMap<&'static str, Replica>,
    pub(super) wire: BTreeMap<&'static str, Vec<Vec<u8>>>,
    pub(super) grants: Vec<u8>,
    pub(super) post: BTreeMap<&'static str, String>,
    pub(super) b_blocks: BTreeSet<String>,
    pub(super) c_did: String,
}
#[derive(Debug, PartialEq)]
pub(super) struct Fingerprint {
    pub(super) snapshots: BTreeMap<&'static str, Snapshot>,
    pub(super) wire: BTreeMap<&'static str, Vec<Vec<u8>>>,
    pub(super) grants: Vec<u8>,
    pub(super) post: BTreeMap<&'static str, String>,
}
impl Run {
    pub(super) fn fingerprint(&self) -> Fingerprint {
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
    pub(super) fn id(&self, name: &str) -> &String {
        &self.post[name]
    }
}
pub(super) fn run(host_is_blocker: bool, block: bool, relay: HostRelay) -> Run {
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
pub(super) enum Display {
    Shown,
    Hidden,
    Collapsed,
}
#[derive(Debug, PartialEq)]
pub(super) enum Line {
    Message { author: String, text: String },
    // Placeholder for a run of consecutive blocked messages; carries no author text.
    Collapsed { count: usize },
}
// Pure view function: reads the replica, never mutates it.
pub(super) fn render(r: &Replica, blocks: &BTreeSet<String>, display: Display) -> Vec<Line> {
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
pub(super) fn texts(lines: &[Line]) -> Vec<&str> {
    lines
        .iter()
        .filter_map(|l| match l {
            Line::Message { text, .. } => Some(text.as_str()),
            Line::Collapsed { .. } => None,
        })
        .collect()
}
