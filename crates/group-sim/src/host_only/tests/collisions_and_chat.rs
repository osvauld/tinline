use super::*;

// KIND: limitation
// Why a writer-ID collision rule is mandatory: two Loro docs that use the same peer ID for
// different content (as two DIDs, or two cloned devices, with a colliding writer ID would)
// import each other's updates without error but silently keep only their own op for the
// shared (peer, counter) — the replicas diverge permanently. Loro alone does not detect it.
#[test]
fn loro_same_peer_id_from_two_writers_silently_diverges_without_a_collision_rule() {
    let mut rng = SimRng(7);
    let shared = rng.writer();
    let (x, y) = (LoroDoc::new(), LoroDoc::new());
    x.set_peer_id(shared).unwrap();
    y.set_peer_id(shared).unwrap();
    x.get_map("messages").insert("k", "from alice").unwrap();
    x.commit();
    y.get_map("messages").insert("k2", "from carol").unwrap();
    y.commit();
    let ux = x.export(ExportMode::all_updates()).unwrap();
    let uy = y.export(ExportMode::all_updates()).unwrap();
    let _ = x.import(&uy);
    let _ = y.import(&ux);
    assert_ne!(x.get_deep_value(), y.get_deep_value(), "replicas diverged");
    let both = LoroDoc::new();
    both.get_map("messages").insert("k", "from alice").unwrap();
    both.get_map("messages").insert("k2", "from carol").unwrap();
    assert_ne!(x.get_deep_value(), both.get_deep_value());
    assert_ne!(y.get_deep_value(), both.get_deep_value());
}

// KIND: candidate-policy G-FORK
// Cross-DID writer-ID collision (forced: Carol reuses Alice-1's writer ID — models a 2^-64
// draw or a broken RNG; still not key-derived). Candidate "first claim wins at the host":
// the host binds writer -> DID on first accept; the later DID's post is refused with the
// host board unchanged. Because all traffic is serialised by the single host, every member
// converges to the host's choice in BOTH arrival orders. The loser redraws a writer ID,
// rebuilds from its validated board and re-posts; all converge with both texts.
#[test]
fn cross_did_writer_collision_first_claim_at_host_wins_and_loser_redraws() {
    for alice_first in [true, false] {
        let mut w = world();
        let shared = w.d[A1].w.writer;
        let carol_id = w.d[CAROL].id.clone();
        w.d[CAROL].w = Writer::new(carol_id, shared);
        w.post(A1, "alice");
        w.post(CAROL, "carol");
        let (first, second) = if alice_first {
            (A1, CAROL)
        } else {
            (CAROL, A1)
        };
        w.reconnect(first).unwrap();
        let hb = w.d[HOST].board.snapshot();
        let hep = w.reach_host(second).unwrap();
        let v = w.upload(second, hep);
        assert_eq!(v, vec![Verdict::Rejected(Why::WriterOwnedByOtherDid)]);
        assert_eq!(w.d[HOST].board.snapshot(), hb);
        for i in [A1, A2, BOB, CAROL] {
            w.reconnect(i).unwrap();
        }
        w.assert_converged(&ALL);
        assert_eq!(w.d[HOST].board.writers[&shared], w.did(first));
        assert_eq!(w.d[HOST].board.accepted.len(), 1);
        let new = w.redraw_writer(second);
        assert_ne!(new, shared);
        w.reconnect(second).unwrap();
        w.assert_converged(&ALL);
        assert_eq!(
            w.d[BOB].board.texts(),
            ["alice".to_string(), "carol".to_string()].into()
        );
    }
}

// KIND: candidate-policy G-FORK
// Same-DID writer-ID collision (forced: Alice-2 reuses Alice-1's writer ID, as a cloned or
// restored device state would). Both post seq 1 with different text.
// - RejectLater: host keeps the first, refuses the second with the host board byte-identical;
//   Alice-1 continues under its writer; Alice-2 redraws and re-posts.
// - FreezeWriter: host refuses the second AND freezes the writer (content state unchanged,
//   only the frozen set grows); Alice-1's next post is refused too, so BOTH devices must
//   redraw. Any phrase holder can therefore freeze a sibling's writer (cf. W2-R Q19).
// Members never receive the conflicting post under either candidate; all converge.
#[test]
fn same_did_writer_collision_reject_later_vs_freeze_writer() {
    for fork in [ForkPolicy::RejectLater, ForkPolicy::FreezeWriter] {
        let mut w = World::new(Cfg {
            fork,
            ..Cfg::default()
        });
        let shared = w.d[A1].w.writer;
        let alice = w.d[A2].id.clone();
        w.d[A2].w = Writer::new(alice, shared);
        w.post(A1, "phone");
        w.post(A2, "cloned laptop");
        w.reconnect(A1).unwrap();
        let (full, content) = (
            w.d[HOST].board.snapshot(),
            w.d[HOST].board.content_snapshot(),
        );
        let hep = w.reach_host(A2).unwrap();
        assert_eq!(w.upload(A2, hep), vec![Verdict::Rejected(Why::SameDidFork)]);
        assert_eq!(w.d[HOST].board.content_snapshot(), content);
        match fork {
            ForkPolicy::RejectLater => assert_eq!(w.d[HOST].board.snapshot(), full),
            ForkPolicy::FreezeWriter => {
                assert_eq!(w.d[HOST].board.frozen, [shared].into())
            }
        }
        for i in [A1, A2, BOB, CAROL] {
            w.reconnect(i).unwrap();
        }
        assert!(!w.d[BOB].board.texts().contains("cloned laptop"));
        w.post(A1, "phone again");
        let hep = w.reach_host(A1).unwrap();
        let v = w.upload(A1, hep);
        match fork {
            ForkPolicy::RejectLater => assert_eq!(v, vec![Verdict::Accepted]),
            ForkPolicy::FreezeWriter => {
                assert_eq!(v, vec![Verdict::Rejected(Why::WriterFrozen)]);
                w.redraw_writer(A1);
            }
        }
        w.redraw_writer(A2);
        for i in [A1, A2, BOB, CAROL] {
            w.reconnect(i).unwrap();
        }
        w.assert_converged(&ALL);
        assert_eq!(
            w.d[CAROL].board.texts(),
            ["phone", "cloned laptop", "phone again"]
                .map(String::from)
                .into()
        );
    }
}

// KIND: invariant
// U3 under U7 + U4 revised: Alice-1 requests a 1:1 chat with Carol via the host. The sealed
// request carries no endpoint and is DID-signed inside the ciphertext; the host routes it by
// header only. A tampered or replayed request is dropped with Carol's state unchanged. Carol
// accepts; the acceptance (DID-signed, sealed to Alice) carries Carol's private endpoint;
// Alice-1 dials Carol directly with no host frame, and Carol learns Alice's private endpoint
// from the DID-signed direct hello. The host and Bob never see either private endpoint.
// Group posts keep working alongside. (Alice-2 drops the acceptance: it has no outgoing
// record — linked-device sync of chat state is out of scope.)
#[test]
fn chat_request_via_host_with_did_signed_content_then_direct() {
    let mut w = world();
    let (alice, carol) = (w.did(A1), w.did(CAROL));
    w.post(A1, "group still works");
    w.reconnect(A1).unwrap();
    let rid = w.chat_request(A1, &carol).unwrap();
    let to_carol: Vec<Frame> = w
        .frames
        .iter()
        .filter(|f| f.kind == "chat" && f.to == CAROL)
        .cloned()
        .collect();
    assert_eq!(to_carol.len(), 1);
    assert_eq!(
        w.d[CAROL].incoming[&rid].0, alice,
        "carol holds the request from alice"
    );
    // The request plaintext has no endpoint of Alice.
    let req: Sealed = serde_json::from_slice(&to_carol[0].bytes).unwrap();
    let plain = open(&StaticSecret::from(w.d[CAROL].req_secret), &req).unwrap();
    assert!(plain.verify());
    assert!(matches!(plain.inner, Inner::Request { .. }));
    let plain_bytes = serde_json::to_vec(&plain).unwrap();
    assert!(!leaks_key(&plain_bytes, &tkey(&w.d[A1].private_key)));
    assert!(!leaks_key(&plain_bytes, &tkey(&w.d[A1].group_key)));
    // Tampered and replayed requests: dropped, Carol unchanged.
    let snap = w.d[CAROL].chat_snapshot();
    let mut t = req.clone();
    t.ciphertext[3] ^= 1;
    let mut rerouted = req.clone();
    rerouted.header.from = w.did(BOB);
    for bytes in [
        serde_json::to_vec(&t).unwrap(),
        serde_json::to_vec(&rerouted).unwrap(),
        to_carol[0].bytes.clone(),
    ] {
        assert_eq!(w.chat_inbox(CAROL, &bytes), ChatIn::Dropped);
        assert_eq!(w.d[CAROL].chat_snapshot(), snap);
    }
    let mut answered = w.chat_accept(CAROL, &rid);
    answered.sort_by_key(|(i, _)| *i);
    assert_eq!(
        answered,
        vec![
            (
                A1,
                ChatIn::Accepted {
                    from: carol.clone()
                }
            ),
            (A2, ChatIn::Dropped)
        ]
    );
    assert_eq!(w.d[A1].private_book[&carol], w.d[CAROL].private_ep());
    let n = w.frames.len();
    w.direct(A1, &carol, "hi carol, directly").unwrap();
    assert!(w.frames[n..].iter().all(|f| f.from != HOST && f.to != HOST));
    assert_eq!(w.d[CAROL].private_book[&alice], w.d[A1].private_ep());
    assert_eq!(w.dial(CAROL, &alice), Ok(w.d[A1].private_ep()));
    // Host and bystander never see a private endpoint.
    for who in [HOST, BOB] {
        let mut hay = w.received(who);
        if who == HOST {
            hay.extend(w.host_snapshot());
        }
        for i in [A1, CAROL] {
            assert!(
                !leaks_key(&hay, &tkey(&w.d[i].private_key)),
                "{who} saw {i}"
            );
        }
    }
    w.assert_converged(&[HOST, A1]);
}

// KIND: limitation
// U7 consequences the host-only model does NOT hide: (1) the host holds every member device's
// group endpoint and route slot, so it can count each DID's devices and see when each device
// rotates; (2) it learns who requested a chat with whom and when (header metadata); (3)
// members learn each DID's device count and per-device activity from the random writer IDs
// in content, even though they never learn device keys or endpoints.
#[test]
fn host_holds_all_member_endpoints_and_metadata_members_see_writer_ids() {
    let mut w = world();
    let (alice, carol) = (w.did(A1), w.did(CAROL));
    let alice_entries = w
        .directory
        .values()
        .filter(|n| n.claims.owner == alice)
        .count();
    assert_eq!(alice_entries, 2);
    for i in [A1, A2, BOB, CAROL] {
        assert_eq!(
            w.directory[&dir_key(&w.did(i), w.d[i].slot)]
                .claims
                .endpoint,
            w.d[i].group_ep()
        );
    }
    w.chat_request(BOB, &carol).unwrap();
    let meta: Vec<Header> = w
        .frames
        .iter()
        .filter(|f| f.to == HOST && f.kind == "chat")
        .map(|f| serde_json::from_slice::<Sealed>(&f.bytes).unwrap().header)
        .collect();
    assert_eq!(
        (meta[0].from.as_str(), meta[0].to.as_str()),
        (w.did(BOB).as_str(), carol.as_str())
    );
    w.post(A1, "one");
    w.post(A2, "two");
    for i in [A1, A2, CAROL] {
        w.reconnect(i).unwrap();
    }
    let writers_seen_by_carol: BTreeSet<u64> = w.d[CAROL]
        .board
        .accepted
        .values()
        .filter(|p| p.body.author == alice)
        .map(|p| p.body.writer)
        .collect();
    assert_eq!(
        writers_seen_by_carol.len(),
        2,
        "carol can count alice's devices"
    );
}
