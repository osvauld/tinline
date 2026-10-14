use super::*;

pub(crate) fn resign(e: &mut Envelope, key: &SigningKey) {
    e.signature = key.sign(&e.bytes()).to_bytes();
}
// Topological order over the given envelopes (deps inside the set come first; ties by ID).
pub(crate) fn causal_order(mut rest: Vec<Envelope>) -> Vec<Envelope> {
    rest.sort_by_key(|e| e.id());
    let inside: BTreeSet<String> = rest.iter().map(|e| e.id()).collect();
    let mut done = BTreeSet::new();
    let mut out = vec![];
    while !rest.is_empty() {
        let i = rest
            .iter()
            .position(|e| {
                e.body
                    .previous
                    .iter()
                    .chain(e.body.dependencies.iter())
                    .all(|d| !inside.contains(d) || done.contains(d))
            })
            .expect("dependency cycle");
        let e = rest.remove(i);
        done.insert(e.id());
        out.push(e);
    }
    out
}

// Repair source that omits one envelope: rebuilds from the oracle without it.
pub(crate) fn oracle_without(oracle: &Replica, omit: &Envelope) -> Replica {
    let mut r = Replica::new(&oracle.group, &[]);
    r.authorization = Box::new(FixtureAllowlist(
        oracle
            .accepted
            .values()
            .map(|e| e.body.author.clone())
            .collect(),
    ));
    for e in causal_order(oracle.accepted.values().cloned().collect()) {
        if e.id() != omit.id() {
            r.receive(e);
        }
    }
    r
}

pub(crate) fn signed_fixture() -> (Genesis, Author, Author, Author) {
    let mut a = Author::new(1, "unconfigured");
    let mut b = Author::new(2, "unconfigured");
    let mut c = Author::new(3, "unconfigured");
    let g = Genesis::create(&a.id, [42; 32]);
    let admin = Grant::issue(
        &a.id,
        b.id.did(),
        &g.id(),
        &[Capability::Read, Capability::Post, Capability::Delegate],
        true,
        1,
        None,
    );
    let member = Grant::issue(
        &b.id,
        c.id.did(),
        &g.id(),
        &[Capability::Read, Capability::Post],
        false,
        2,
        Some(admin.clone()),
    );
    let owner = Grant::issue(
        &a.id,
        a.id.did(),
        &g.id(),
        &[Capability::Read, Capability::Post, Capability::Delegate],
        true,
        3,
        None,
    );
    a.configure(&g.id(), owner);
    b.configure(&g.id(), admin);
    c.configure(&g.id(), member);
    (g, a, b, c)
}

// Hand-built envelope from DID `owner` using an additional device key, writing under
// Loro peer `writer` (normally peer(device, doc)). Legacy fixture, still used by
// `policy_candidates.rs` for second-device chains.
pub(crate) fn second_device_envelope(
    owner: &Author,
    device: &SigningKey,
    writer: u64,
    text: &str,
) -> Envelope {
    let doc = LoroDoc::new();
    doc.set_peer_id(writer).unwrap();
    let value = serde_json::to_string(&(owner.id.did(), text)).unwrap();
    doc.get_map("messages")
        .insert(&format!("{writer}:1"), value)
        .unwrap();
    doc.commit();
    let mut e = Envelope {
        body: Body {
            group: owner.group.clone(),
            doc: owner.doc_name.clone(),
            author: owner.id.did().into(),
            device: device.verifying_key().to_bytes(),
            peer: writer,
            sequence: 1,
            previous: None,
            dependencies: BTreeSet::new(),
            permit: None,
            text: text.into(),
            update: doc.export(ExportMode::all_updates()).unwrap(),
        },
        attestation: proto::attest(&owner.id, device.verifying_key().to_bytes(), 0),
        signature: [0; 64],
    };
    resign(&mut e, device);
    e
}
pub(crate) fn did(a: &Author) -> String {
    a.id.did().to_string()
}
