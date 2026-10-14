use super::*;

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
