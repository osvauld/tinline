use super::*;

// KIND: candidate-policy S06
// PersistBounded: pending originals are written durably (still not accepted), re-verified on
// open, and the host itself names the missing dependency to re-request. A corrupted persisted
// pending record is quarantined, never promoted, and is recovered from the sender's original.
#[test]
fn s06_persisted_pending_queue_is_reverified_and_rerequested_after_restart() {
    let (g, a1, later, mut host) = s06_setup(PendingPolicy::PersistBounded, 2);
    let control = s06_control(&g, &a1, &later, &host);
    let mut net = Net::default();
    let mut sender = Sender::new(&later.iter().collect::<Vec<_>>());
    let writes = host.durable.writes;
    for e in &later {
        assert_eq!(
            host.ingest(e.clone(), Fault::None, &mut net),
            Ingest::Pending
        );
    }
    assert_eq!(host.durable.writes, writes + 2);
    assert_eq!(host.durable.pending.len(), 2);
    assert!(host.durable.originals.len() == 1 && net.acks.is_empty());
    host.crash();
    assert!(host.open(OpenPolicy::FailClosed).unwrap().is_clean());
    assert_eq!(host.replica().pending.len(), 2);
    assert_eq!(host.missing_dependencies(), BTreeSet::from([a1.id()]));
    let all: BTreeSet<String> = std::iter::once(&a1).chain(&later).map(|e| e.id()).collect();
    assert_eq!(
        host.ingest(a1.clone(), Fault::None, &mut net),
        Ingest::Stored(all)
    );
    assert!(host.durable.pending.is_empty());
    assert_eq!(canonical(&host.durable), control);
    sender.absorb(&net);
    assert!(sender.unacked.is_empty());

    // Corrupted persisted pending record (signature flip on the last chain element).
    let (g, a1, later, mut host) = s06_setup(PendingPolicy::PersistBounded, 2);
    let mut sender = Sender::new(&later.iter().collect::<Vec<_>>());
    let control = s06_control(&g, &a1, &later, &host);
    let mut net = Net::default();
    for e in &later {
        assert_eq!(
            host.ingest(e.clone(), Fault::None, &mut net),
            Ingest::Pending
        );
    }
    host.crash();
    let target = later[1].id();
    let mut bad = decode(&host.durable.pending[&target]).unwrap();
    bad.signature[0] ^= 1;
    host.durable.pending.insert(target.clone(), encode(&bad));
    let report = host.open(OpenPolicy::Reconstruct).unwrap();
    assert_eq!(report.quarantined_pending, BTreeSet::from([target.clone()]));
    assert_eq!(
        host.ingest(a1.clone(), Fault::None, &mut net),
        Ingest::Stored(BTreeSet::from([a1.id(), later[0].id()]))
    );
    assert!(!host.replica().accepted.contains_key(&target));
    assert_eq!(
        sender.retry(&mut host, &mut net),
        vec![Ingest::Stored(BTreeSet::from([target]))]
    );
    assert_eq!(canonical(&host.durable), control);
}
pub(super) fn fresh_host(g: &Genesis) -> Host {
    Host::new(g, AckOrder::CommitThenAck, PendingPolicy::Volatile)
}

// a1; b1 saw a1; a2; c1 saw a1+b1; b2. Peer P got an earlier overlapping batch [a1, b1, a2].
pub(super) struct S07 {
    pub(super) g: Genesis,
    pub(super) es: Vec<Envelope>,
    pub(super) a: Author,
}
pub(super) fn s07_fixture() -> S07 {
    let (g, mut a, mut b, mut c) = signed_fixture();
    let a1 = a.post("a1");
    b.observe(&a1);
    let b1 = b.post("b1");
    let a2 = a.post("a2");
    c.observe(&a1);
    c.observe(&b1);
    let c1 = c.post("c1");
    let b2 = b.post("b2");
    S07 {
        g,
        es: vec![a1, b1, a2, c1, b2],
        a,
    }
}
pub(super) fn s07_host_and_peer(fx: &S07) -> (Host, Replica) {
    let mut host = fresh_host(&fx.g);
    let mut net = Net::default();
    for e in &fx.es {
        assert!(host.ingest(e.clone(), Fault::None, &mut net).is_stored());
    }
    // Export happens after a restart: only persisted originals are available.
    host.crash();
    assert!(host.open(OpenPolicy::FailClosed).unwrap().is_clean());
    let mut peer = fresh(&fx.g);
    for e in &fx.es[..3] {
        assert_eq!(peer.receive(e.clone()), Outcome::Accepted);
    }
    (host, peer)
}

// KIND: invariant
// S07: export after overlap sends exactly the missing ops as stored original bytes (author
// signatures intact); resending the whole overlapping set is idempotent.
#[test]
fn s07_export_after_overlapping_batch_sends_only_missing_byte_identical_originals() {
    let fx = s07_fixture();
    let (host, mut peer) = s07_host_and_peer(&fx);
    let known: BTreeSet<String> = peer.accepted.keys().cloned().collect();
    let exported = host.export_missing(&known).unwrap();
    let expected: BTreeSet<Vec<u8>> = fx.es[3..].iter().map(encode).collect();
    assert_eq!(exported.iter().cloned().collect::<BTreeSet<_>>(), expected);
    for bytes in &exported {
        assert!(host.durable.originals.values().any(|o| o == bytes));
        let e = decode(bytes).unwrap();
        let original = fx.es.iter().find(|o| o.id() == e.id()).unwrap();
        assert_eq!(e.signature, original.signature);
        assert_eq!(e.body.author, original.body.author);
    }
    let outcomes: Vec<Outcome> = exported
        .iter()
        .map(|b| peer.receive(decode(b).unwrap()))
        .collect();
    assert_eq!(outcomes, vec![Outcome::Accepted, Outcome::Accepted]);
    assert_eq!(view(&peer), view(host.replica()));
    assert_eq!(peer.fanout.len(), 5);
    let settled = view(&peer);
    for bytes in host.export_missing(&BTreeSet::new()).unwrap() {
        assert_eq!(peer.receive(decode(&bytes).unwrap()), Outcome::Duplicate);
    }
    assert_eq!(view(&peer), settled);
    assert_eq!(peer.fanout.len(), 5);
}

// KIND: invariant
// S07 negative control: a diff synthesized from host state is refused, whether it reuses an
// author's signature (relabelled) or is freshly signed by a member over other authors' ops.
// Each input differs from a valid one in one respect; the populated peer is unchanged.
#[test]
fn s07_synthesized_diff_from_host_state_is_rejected_and_leaves_peer_unchanged() {
    let mut fx = s07_fixture();
    let (host, mut peer) = s07_host_and_peer(&fx);
    let diff = host
        .replica()
        .doc
        .export(ExportMode::updates(&peer.doc.oplog_vv()))
        .unwrap();
    // (i) c1's original with its update replaced by the synthesized diff; c1's signature kept.
    let mut relabelled = fx.es[3].clone();
    relabelled.body.update = diff.clone();
    // (ii) member A signs a well-formed next envelope whose update carries B's and C's ops.
    let a = &mut fx.a;
    let p = peer_of(a);
    let mut signed = Envelope {
        body: Body {
            group: a.group.clone(),
            doc: a.doc_name.clone(),
            author: a.id.did().into(),
            device: a.key.verifying_key().to_bytes(),
            peer: p,
            sequence: a.sequence + 1,
            previous: a.previous.clone(),
            dependencies: a.observed.clone(),
            permit: a.permit.clone(),
            text: "synthesized".into(),
            update: diff,
        },
        attestation: a.attestation.clone(),
        signature: [0; 64],
    };
    signed.signature = a.key.sign(&signed.bytes()).to_bytes();
    assert!(peer.valid_proof(&signed) && peer.dependencies_ready(&signed));
    let before = view(&peer);
    let fanout = peer.fanout.len();
    for e in [relabelled, signed] {
        assert_eq!(peer.receive(e), Outcome::Invalid);
        assert_eq!(view(&peer), before);
        assert_eq!(peer.fanout.len(), fanout);
    }
    // Positive control: the real originals are accepted.
    for e in &fx.es[3..] {
        assert_eq!(peer.receive(e.clone()), Outcome::Accepted);
    }
}
pub(super) fn peer_of(a: &Author) -> u64 {
    peer(&a.key.verifying_key().to_bytes(), &a.doc_name)
}
