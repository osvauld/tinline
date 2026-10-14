use super::*;

// ============================================================================================
// Tests
// ============================================================================================

// KIND: invariant
// GC-S01. H refuses each bad Join on a populated call: a non-member, a non-member host binary,
// a forged signature (tampered body, wrong signer), a wrong gid, a wrong call_id, a replay, an
// endpoint that does not match, an oversize frame, a removed member (U12: immediate) and a
// stale host_gen after a redial. Every device's state, H included, stays byte-identical.
// Positive controls are a valid Join on the same call and a valid Join at the new generation.
#[test]
fn gc_s01_bad_joins_are_refused_and_leave_every_state_byte_identical() {
    let (mut s, cid) = base();
    for m in [A1, BOB] {
        s.join_shown(m).unwrap();
    }
    s.advance(3_000);
    let gid = s.w.gid.clone();
    let hgen = 1;
    let mallory = ident(37);
    let m_key = SigningKey::from_bytes(&[9; 32]);
    let m_ep = ep("grp", &m_key);
    let m_note = EpNote::sign(&mallory, Audience::Group(gid.clone()), 77, &m_ep, 1);
    let carol_ep = s.w.gep(CAROL);
    let carol_note = s.w.d[CAROL].my_note.clone();
    let mk = |s: &Sim,
              id: &Identity,
              signer: String,
              call_id: &str,
              gid: &str,
              hgen: u32,
              nonce: u64| {
        let m = CallMsg {
            v: 1,
            gid: gid.into(),
            call_id: call_id.into(),
            signer,
            host_gen: hgen,
            kind: Kind::Join {
                issued_ms: s.now,
                nonce,
            },
        };
        Ctl::sign(id, m)
    };
    let req = |j: Ctl, n: &EpNote| {
        serde_json::to_vec(&JoinReq {
            join: j,
            note: n.clone(),
        })
        .unwrap()
    };
    let carol = s.w.d[CAROL].id.clone();
    let carol_did = s.did(CAROL);
    let mut tampered = mk(&s, &carol, carol_did.clone(), &cid, &gid, hgen, 1);
    tampered.msg.kind = Kind::Join {
        issued_ms: s.now,
        nonce: 2,
    };
    let wrong_signer = mk(&s, &mallory, carol_did.clone(), &cid, &gid, hgen, 3);
    let hugo_note = s.w.d[H2].my_note.clone();
    let hugo = s.w.d[H2].id.clone();
    let accepted_a1 = s.c[H1].hosting.as_ref().unwrap().parts[&s.did(A1)]
        .join
        .clone();
    let a1_note = s.w.d[A1].my_note.clone();
    let a1_ep = s.w.gep(A1);
    let bob_join = mk(&s, &s.w.d[BOB].id.clone(), s.did(BOB), &cid, &gid, hgen, 4);
    let bob_note = s.w.d[BOB].my_note.clone();
    let mut oversize = req(
        mk(&s, &carol, carol_did.clone(), &cid, &gid, hgen, 5),
        &carol_note,
    );
    oversize.extend(vec![b' '; MAX_CONTROL]);
    let cases: Vec<(&str, String, Vec<u8>, CR)> = vec![
        (
            "non-member",
            m_ep.clone(),
            req(
                mk(&s, &mallory, mallory.did().into(), &cid, &gid, hgen, 6),
                &m_note,
            ),
            CR::NotMember,
        ),
        (
            "non-member host binary",
            s.w.gep(H2),
            req(
                mk(&s, &hugo, hugo.did().into(), &cid, &gid, hgen, 7),
                &hugo_note,
            ),
            CR::NotMember,
        ),
        (
            "tampered body",
            carol_ep.clone(),
            req(tampered, &carol_note),
            CR::BadSignature,
        ),
        (
            "signed by another key",
            carol_ep.clone(),
            req(wrong_signer, &carol_note),
            CR::BadSignature,
        ),
        (
            "wrong gid",
            carol_ep.clone(),
            req(
                mk(
                    &s,
                    &carol,
                    carol_did.clone(),
                    &cid,
                    &"f".repeat(64),
                    hgen,
                    8,
                ),
                &carol_note,
            ),
            CR::WrongGroup,
        ),
        (
            "wrong call_id",
            carol_ep.clone(),
            req(
                mk(
                    &s,
                    &carol,
                    carol_did.clone(),
                    &"0".repeat(32),
                    &gid,
                    hgen,
                    9,
                ),
                &carol_note,
            ),
            CR::WrongCall,
        ),
        (
            "future host_gen",
            carol_ep.clone(),
            req(
                mk(&s, &carol, carol_did.clone(), &cid, &gid, hgen + 1, 10),
                &carol_note,
            ),
            CR::StaleGen,
        ),
        (
            "replayed accepted Join",
            a1_ep.clone(),
            req(accepted_a1, &a1_note),
            CR::Replay,
        ),
        (
            "bob's Join from carol's connection",
            carol_ep.clone(),
            req(bob_join, &bob_note),
            CR::EndpointMismatch,
        ),
        ("oversize", carol_ep.clone(), oversize, CR::Oversize),
    ];
    let roster = roster_of(&s, H1);
    for (case, conn, bytes, want) in cases {
        let before = s.snap_all();
        assert_eq!(s.host_join(H1, &conn, &bytes), Err(want), "{case}");
        assert_unchanged(&s, &before, case);
        assert_eq!(roster_of(&s, H1), roster, "{case}");
    }
    // Positive control: carol's valid Join on the same call.
    s.join_shown(CAROL).unwrap();
    assert_eq!(roster_of(&s, H1).len(), 3);
    // Removed member (U12 detail: refusal is immediate). H ejects her and refuses a re-Join.
    s.remove_member(CAROL, &[OLGA, A1, A2, BOB, CAROL, HP, H1, H2]);
    assert!(s.c[CAROL].joined.is_none());
    s.advance(3_000);
    let before = s.snap_all();
    let again = s.join_req(CAROL, &cid, hgen, s.now);
    assert_eq!(s.host_join(H1, &carol_ep, &again), Err(CR::NotMember));
    assert_unchanged(&s, &before, "removed member");
    // Stale host_gen after a redial. H1 drops, members move to BOB at gen 2, and a gen-1 Join
    // reaches BOB.
    s.w.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.c[BOB].hosting.as_ref().unwrap().host_gen, 2);
    let old = s.join_req(OLGA, &cid, 1, s.now);
    let before = s.snap_all();
    assert_eq!(s.host_join(BOB, &s.w.gep(OLGA), &old), Err(CR::StaleGen));
    assert_unchanged(&s, &before, "stale host_gen");
    // Positive control at gen 2.
    let ok = s.join_req(OLGA, &cid, 2, s.now);
    s.c[OLGA].dialing = Some(Dialing {
        call_id: cid.clone(),
        host: s.host_ref(BOB),
        host_ep: s.w.gep(BOB),
        host_gen: 2,
    });
    assert!(s.host_join(BOB, &s.w.gep(OLGA), &ok).is_ok());
}

// KIND: invariant (ignored: not expressible)
// GC-S01, §3.2 item 4, second half: "the device slot is not on the DID's signed device list as
// unlinked".
#[test]
#[ignore = "GC-S01: the multi_host fixture has no signed device list (U8), so an unlinked slot cannot be expressed; the Join also carries no device slot apart from the group note's"]
pub(super) fn gc_s01b_join_from_a_device_slot_marked_unlinked_is_refused() {
    unimplemented!("needs a signed device-list fixture (crates/proto rotation_device_list_push)");
}

// KIND: invariant
// GC-S02 / R05. Members refuse each bad CallStart and show no call. The cases are a chat-host
// permit without `CallHost`, a revoked call permit, a non-member host binary with a valid
// CallHost permit, a sibling device's permit, a permit issued by a non-admin, a forged
// signature, a wrong gid and a host_ref that does not match. Every device stays byte-identical.
// The positive control is a valid CallStart, which is shown.
#[test]
fn gc_s02_callstart_without_a_valid_call_permit_is_refused_and_no_call_is_shown() {
    let mut s = Sim::new();
    let cid = s.new_call_id();
    let gid = s.w.gid.clone();
    let announce = |s: &Sim, h: usize, permit: MGrant, gid: &str, f: &dyn Fn(&mut CallMsg)| {
        let mut start = s.start_msg(h, &cid, 1, OLGA, vec![]);
        start.msg.gid = gid.into();
        let rf = HostRef {
            holder_did: s.did(h),
            device_slot: s.w.d[h].slot,
            permit_digest: permit.digest(),
        };
        if let Kind::Start { host_ref, .. } = &mut start.msg.kind {
            *host_ref = rf;
        }
        f(&mut start.msg);
        let start = Ctl::sign(&s.w.d[h].id, start.msg);
        let record = HostRecord {
            note: s.w.d[h].my_note.clone(),
            permit,
        };
        serde_json::to_vec(&Announce { start, record }).unwrap()
    };
    let none = |_: &mut CallMsg| {};
    let chat_permit = s.w.d[H1].host.as_ref().unwrap().record.permit.clone();
    let hugo_call = s.call_permit(OLGA, H2, s.w.d[H2].slot);
    let sibling = s.permits[&HP].clone();
    let carol_made = MGrant::issue(
        &s.w.d[CAROL].id,
        &gid,
        &s.did(H1),
        Some(s.w.d[H1].slot),
        &[Cap::CallHost],
        false,
        90,
        None,
    );
    let good = s.permits[&H1].clone();
    let mut forged = announce(&s, H1, good.clone(), &gid, &none);
    let at = forged.windows(4).position(|w| w == b"\"k\":").unwrap();
    forged[at + 4] = b'9';
    let mismatched = {
        let mut a: Announce =
            serde_json::from_slice(&announce(&s, H1, good.clone(), &gid, &none)).unwrap();
        if let Kind::Start { host_ref, .. } = &mut a.start.msg.kind {
            host_ref.device_slot ^= 1;
        }
        a.start = Ctl::sign(&s.w.d[H1].id, a.start.msg);
        serde_json::to_vec(&a).unwrap()
    };
    let cases: Vec<(&str, Vec<u8>, CR)> = vec![
        (
            "chat host without host_call (R05)",
            announce(&s, H1, chat_permit, &gid, &none),
            CR::PermitInvalid,
        ),
        (
            "non-member host binary with a CallHost permit",
            announce(&s, H2, hugo_call, &gid, &none),
            CR::NotMember,
        ),
        (
            "sibling device's permit",
            announce(&s, H1, sibling, &gid, &none),
            CR::DeviceMismatch,
        ),
        (
            "permit issued by a non-admin",
            announce(&s, H1, carol_made, &gid, &none),
            CR::PermitInvalid,
        ),
        ("forged signature", forged, CR::BadSignature),
        (
            "wrong gid",
            announce(&s, H1, good.clone(), &"e".repeat(64), &none),
            CR::WrongGroup,
        ),
        (
            "host_ref does not match the record",
            mismatched,
            CR::DeviceMismatch,
        ),
    ];
    let members = [OLGA, A1, A2, BOB, CAROL, HP];
    for (case, bytes, want) in cases {
        let before = s.snap_all();
        for &m in &members {
            assert_eq!(
                s.accept_start(m, &bytes),
                Err(want.clone()),
                "{case} at {m}"
            );
            assert!(s.c[m].shown.is_none(), "{case}");
        }
        assert_unchanged(&s, &before, case);
    }
    // Revoked: an admin revokes H1's call permit, then H1's otherwise valid CallStart is refused.
    let mut r = Sim::new();
    let ok = {
        let st = r.start_msg(H1, &cid, 1, OLGA, vec![]);
        serde_json::to_vec(&Announce {
            start: st,
            record: r.record(H1),
        })
        .unwrap()
    };
    r.revoke(A1, H1, &[OLGA, A1, A2, BOB, CAROL, HP, H1]);
    let before = r.snap_all();
    for m in members {
        assert_eq!(r.accept_start(m, &ok), Err(CR::Revoked), "revoked at {m}");
    }
    assert_unchanged(&r, &before, "revoked");
    assert_eq!(r.start_call(H1, OLGA, vec![]), Err(CR::Revoked));
    // Positive control: H1's valid CallStart is shown everywhere.
    s.begin_hosting(H1, &cid, 1, OLGA, vec![]).unwrap();
    for m in members {
        assert_eq!(s.c[m].shown.as_deref(), Some(cid.as_str()));
    }
}

// KIND: invariant
// GC-S03. After a redial to gen 2, these are ignored everywhere with state byte-identical:
// the old host's End{ended} at gen 1, a replayed gen-1 CallStart, a freshly signed gen-1
// CallStart and a gen-1 Roster.
#[test]
fn gc_s03_stale_end_and_callstart_after_redial_are_ignored_everywhere() {
    let (mut s, cid) = base();
    for m in [A1, CAROL] {
        s.join_shown(m).unwrap();
    }
    let gen1 = serde_json::to_vec(&Announce {
        start: s.c[H1].hosting.as_ref().unwrap().start.clone(),
        record: s.record(H1),
    })
    .unwrap();
    s.w.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.c[BOB].hosting.as_ref().unwrap().host_gen, 2);
    s.w.d[H1].online = true;
    let stale_end = s.msg(
        H1,
        &cid,
        1,
        Kind::End {
            reason: EndReason::Ended,
            next_host: None,
        },
    );
    let fresh_gen1 = serde_json::to_vec(&Announce {
        start: s.start_msg(H1, &cid, 1, OLGA, vec![]),
        record: s.record(H1),
    })
    .unwrap();
    let stale_roster = s.msg(
        H1,
        &cid,
        1,
        Kind::Roster {
            epoch: 99,
            entries: vec![],
        },
    );
    let all: Vec<usize> = (0..s.w.d.len()).collect();
    let before = s.snap_all();
    for &i in &all {
        if s.c[i].hgen.contains_key(&cid) {
            assert_eq!(
                s.accept_end(i, &stale_end.wire()),
                Err(CR::StaleGen),
                "end at {i}"
            );
            assert_eq!(
                s.accept_start(i, &gen1),
                Err(CR::StaleGen),
                "replayed start at {i}"
            );
            assert_eq!(
                s.accept_start(i, &fresh_gen1),
                Err(CR::StaleGen),
                "fresh gen-1 at {i}"
            );
        }
        if s.c[i].joined.is_some() {
            assert_eq!(
                s.accept_roster(i, &stale_roster.wire()),
                Err(CR::StaleGen),
                "roster at {i}"
            );
        }
    }
    assert_unchanged(&s, &before, "stale gen-1 control");
    for m in [A1, CAROL] {
        assert_eq!(s.c[m].joined.as_ref().unwrap().host_gen, 2);
    }
}

// KIND: invariant
// GC-S04. The starter and every member rank the same status set, given to each in a different
// order, and get the same ranking and backups. Tiers, exclusions, TTL (capped at
// STATUS_TTL_MS), the starter tie-break and the hash tie-break all hold. Forged statuses and
// statuses without a permit are dropped. The picker moves to the next candidate after a refusal,
// or after ANSWER_TIMEOUT_MS when a candidate does not answer.
#[test]
fn gc_s04_same_status_set_gives_identical_ranking_and_backups_everywhere() {
    let mut s = Sim::new();
    let cid = s.new_call_id();
    s.c[HP].cond = Cond::phone(90, true); // phone charging on Wi-Fi: tier 1
    let now = s.now;
    let mut st: Vec<Status> = CALL_HOSTS
        .iter()
        .map(|&h| s.status(h, now - 1_000, STATUS_TTL_MS))
        .collect();
    // A forged status (busy flipped after signing), and one from a non-permit device.
    let mut forged = s.status(BOB, now, STATUS_TTL_MS);
    forged.body.max_participants = 1;
    st.push(forged);
    let mut carol = s.status(BOB, now, STATUS_TTL_MS);
    carol.body.host_ref.holder_did = s.did(CAROL);
    carol.sig = s.w.d[CAROL]
        .id
        .sign(&signed(STATUS_DOMAIN, &carol.body))
        .to_vec();
    st.push(carol);
    let starter = s.did(CAROL);
    let reference = s.rank_at(CAROL, &st, &cid, &starter, 4);
    let mut tier0: Vec<HostRef> = [H1, BOB, OLGA].iter().map(|&h| s.host_ref(h)).collect();
    tier0.sort_by_key(|r| tie_hash(&cid, r));
    let mut expect = tier0.clone();
    expect.push(s.host_ref(HP));
    assert_eq!(
        reference, expect,
        "tier 0 ordered by hash, then the tier-1 phone"
    );
    let mut rng = SimRng(4);
    for m in [OLGA, A1, A2, BOB, HP] {
        let mut shuffled = st.clone();
        for i in (1..shuffled.len()).rev() {
            shuffled.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
        }
        assert_eq!(
            s.rank_at(m, &shuffled, &cid, &starter, 4),
            reference,
            "member {m}"
        );
    }
    let backups: Vec<HostRef> = reference[1..=MAX_BACKUPS].to_vec();
    assert_eq!(backups.len(), 2);
    // The starter itself goes first when it is in the best tier.
    let bob_starts = s.rank_at(CAROL, &st, &cid, &s.did(BOB), 4);
    assert_eq!(bob_starts[0], s.host_ref(BOB));
    // HP as starter is tier 1, so it does not jump ahead of tier 0.
    let hp_starts = s.rank_at(CAROL, &st, &cid, &s.did(HP), 4);
    assert_ne!(hp_starts[0], s.host_ref(HP));
    // Exclusions and tiers (pure).
    let body = |s: &Sim, h: usize, f: &dyn Fn(&mut StatusBody)| {
        let mut b = s.status(h, now, STATUS_TTL_MS).body;
        f(&mut b);
        b
    };
    let rows: Vec<(&str, StatusBody, usize, Option<u8>)> = vec![
        (
            "desktop mains unmetered",
            body(&s, BOB, &|_| {}),
            4,
            Some(0),
        ),
        (
            "phone charging",
            body(&s, HP, &|b| b.power = Power::Mains),
            4,
            Some(1),
        ),
        (
            "phone 50 %",
            body(&s, HP, &|b| b.power = Power::Battery(50)),
            4,
            Some(2),
        ),
        (
            "phone 49 % (gap G7)",
            body(&s, HP, &|b| b.power = Power::Battery(49)),
            4,
            Some(TIER_UNLISTED),
        ),
        (
            "phone 29 %",
            body(&s, HP, &|b| b.power = Power::Battery(29)),
            4,
            None,
        ),
        (
            "phone metered, N = cap",
            body(&s, HP, &|b| b.metered = true),
            CAP_PHONE_METERED,
            Some(3),
        ),
        (
            "phone metered, N > cap",
            body(&s, HP, &|b| b.metered = true),
            CAP_PHONE_METERED + 1,
            None,
        ),
        (
            "thermal not ok",
            body(&s, BOB, &|b| b.thermal_ok = false),
            4,
            None,
        ),
        ("busy", body(&s, BOB, &|b| b.busy = true), 4, None),
        (
            "too small",
            body(&s, BOB, &|b| b.max_participants = 3),
            4,
            None,
        ),
        (
            "desktop on battery (gap G7)",
            body(&s, BOB, &|b| b.power = Power::Battery(90)),
            4,
            Some(TIER_UNLISTED),
        ),
    ];
    for (case, b, n, want) in rows {
        assert_eq!(tier(&b, n), want, "{case}");
    }
    // TTL: stale statuses are dropped. A claimed TTL longer than STATUS_TTL_MS is capped.
    let old = s.status(BOB, now - STATUS_TTL_MS, STATUS_TTL_MS);
    let long = s.status(OLGA, now - STATUS_TTL_MS, 10 * STATUS_TTL_MS);
    let fresh = s.status(H1, now - STATUS_TTL_MS + 1, STATUS_TTL_MS);
    assert_eq!(
        s.rank_at(CAROL, &[old, long, fresh], &cid, &starter, 4),
        vec![s.host_ref(H1)]
    );
    // Picker (§2.3): the first candidate refuses (consent off), the second is offline (no answer
    // within ANSWER_TIMEOUT_MS), and the third hosts.
    let first = s.dev_of(&reference[0]);
    s.c[first].consent = false;
    let second = s.dev_of(&reference[1]);
    s.w.d[second].online = false;
    let mut elapsed = 0;
    let mut host = None;
    for r in &reference {
        let h = s.dev_of(r);
        if !s.w.d[h].online {
            elapsed += ANSWER_TIMEOUT_MS;
            continue;
        }
        let b: Vec<HostRef> = reference
            .iter()
            .filter(|x| *x != r)
            .take(MAX_BACKUPS)
            .cloned()
            .collect();
        if s.start_call(h, CAROL, b).is_ok() {
            host = Some(h);
            break;
        }
    }
    assert_eq!(host, Some(s.dev_of(&reference[2])));
    assert_eq!(elapsed, ANSWER_TIMEOUT_MS);
}

// KIND: invariant
// GC-S05. Two members start a call at the same time on different hosts. The CallStart relay is
// held, so each call gets participants first. After the relay, the lower BLAKE3(call_id) wins
// everywhere, the losing host sends End{superseded, next_host}, and its participants (including
// the losing host's own user) redial the winner. Everyone converges on one call.
#[test]
fn gc_s05_concurrent_starts_converge_on_one_call_and_the_loser_ends_superseded() {
    let mut s = Sim::new();
    s.hold_relay = true;
    let c1 = s.start_call(H1, OLGA, vec![s.host_ref(OLGA)]).unwrap();
    let c2 = s.start_call(BOB, CAROL, vec![s.host_ref(OLGA)]).unwrap();
    // Starters get their host's CallStart directly (the CallHostRequest answer).
    let a1 = serde_json::to_vec(&Announce {
        start: s.c[H1].hosting.as_ref().unwrap().start.clone(),
        record: s.record(H1),
    })
    .unwrap();
    let a2 = serde_json::to_vec(&Announce {
        start: s.c[BOB].hosting.as_ref().unwrap().start.clone(),
        record: s.record(BOB),
    })
    .unwrap();
    for m in [OLGA, A1] {
        s.accept_start(m, &a1).unwrap();
        s.join_shown(m).unwrap();
    }
    for m in [CAROL] {
        s.accept_start(m, &a2).unwrap();
        s.join_shown(m).unwrap();
    }
    s.join_shown(BOB).unwrap(); // BOB's own user in BOB's call
    assert_eq!(roster_of(&s, H1).len(), 2);
    assert_eq!(roster_of(&s, BOB).len(), 2);
    s.flush_relay();
    let (winner, win_host, lose_host) = if call_hash(&c1) < call_hash(&c2) {
        (c1.clone(), H1, BOB)
    } else {
        (c2.clone(), BOB, H1)
    };
    assert!(s.c[lose_host].hosting.is_none(), "loser stopped hosting");
    for m in [OLGA, A1, A2, BOB, CAROL, HP, H1] {
        assert_eq!(
            s.c[m].shown.as_deref(),
            Some(winner.as_str()),
            "shown at {m}"
        );
    }
    for m in [OLGA, A1, BOB, CAROL] {
        let j = s.c[m]
            .joined
            .as_ref()
            .unwrap_or_else(|| panic!("{m} not in a call"));
        assert_eq!(j.call_id, winner, "{m} converged");
        assert_eq!(s.dev_of(&j.host), win_host);
    }
    assert_eq!(roster_of(&s, win_host).len(), 4);
    let superseded = [OLGA, A1, BOB, CAROL]
        .iter()
        .filter(|&&m| s.c[m].log.iter().any(|l| l == "end:Superseded"))
        .count();
    assert_eq!(
        superseded, 2,
        "the losing call's two participants got End{{superseded}}"
    );
}

// KIND: invariant
// GC-S06. Speaker selection for N = 3/5/8/12 and K = 2/3. When every member sends all the time,
// the forwarded counts equal the §4 formulas. H downlink is N-1. H uplink is (N-2)K+1, or
// (N-1)^2 when N <= K, and that is the worst case with H in S. A dedicated host has downlink N
// and uplink NK-K. Each member gets at most K streams and never its own. The §4 table follows
// from the per-stream bytes. With the silence gate and one speaker, the counts are 1 down and
// N-2 up, or 0 down and N-1 up when H speaks. Hysteresis stops flapping, and a sustained louder
// speaker replaces after the average delay plus HYST_FRAMES.
#[test]
fn gc_s06_forwarded_counts_match_section_4_and_hysteresis_prevents_flapping() {
    // (N, K, H up streams, H up kbps, H down kbps, out pps, in pps, MB/h, member down kbps)
    let table: [(usize, usize, usize, u64, u64, u64, u64, u64, u64); 8] = [
        (3, 2, 3, 228, 152, 150, 100, 171, 152),
        (3, 3, 4, 304, 152, 200, 100, 205, 152),
        (5, 2, 7, 532, 304, 350, 200, 376, 152),
        (5, 3, 10, 760, 304, 500, 200, 479, 228),
        (8, 2, 13, 988, 532, 650, 350, 684, 152),
        (8, 3, 19, 1444, 532, 950, 350, 889, 228),
        (12, 2, 21, 1596, 836, 1050, 550, 1094, 152),
        (12, 3, 31, 2356, 836, 1550, 550, 1436, 228),
    ];
    let overhead = DRED_BYTES + AUDIO_HDR + GROUP_HDR + QUIC_BYTES + UDP_IP4;
    let stream_bytes = |bps: u64| bps / 8 / 50 + overhead;
    let kbps = |bps: u64| stream_bytes(bps) * 50 * 8 / 1000;
    assert_eq!(stream_bytes(48_000), 190);
    assert_eq!(kbps(48_000), 76);
    for (n, k, up_s, up_k, down_k, out_pps, in_pps, mbh, mem_k) in table {
        let slots: BTreeSet<u8> = (0..n as u8).collect();
        // H = slot 0, loudest, so H is in S (the worst case).
        let mut f = Fwd::new(k);
        let levels: BTreeMap<u8, u8> = slots.iter().map(|&s| (s, 10 + 2 * s)).collect();
        for _ in 0..AVG_FRAMES * 2 {
            f = select(&f, &levels);
        }
        assert_eq!(f.set.len(), k.min(n));
        assert!(f.set.contains(&0));
        let cp = copies(&f.set, &slots, &slots);
        assert!(cp.iter().all(|(a, b)| a != b), "no self-echo");
        for r in &slots {
            assert!(
                cp.iter().filter(|(_, to)| to == r).count() <= k,
                "member downlink <= K"
            );
        }
        let h_up = cp.iter().filter(|(_, to)| *to != 0).count();
        let h_down = slots.len() - 1;
        let formula = if n > k {
            (n - 2) * k + 1
        } else {
            (n - 1) * (n - 1)
        };
        assert_eq!(h_up, formula, "N={n} K={k}");
        assert_eq!(h_up, up_s, "table row N={n} K={k}");
        let per = kbps(48_000);
        assert_eq!(h_up as u64 * per, up_k);
        assert_eq!(h_down as u64 * per, down_k);
        assert_eq!(h_up as u64 * 50, out_pps);
        assert_eq!(h_down as u64 * 50, in_pps);
        assert_eq!(
            ((h_up + h_down) as u64 * per * 3600 + 4000) / 8000,
            mbh,
            "MB/h N={n} K={k}"
        );
        assert_eq!(k.min(n - 1) as u64 * per, mem_k);
        // H not in S (quietest) costs one stream less, so the formula really is the worst case.
        let mut g = Fwd::new(k);
        let quiet_h: BTreeMap<u8, u8> = slots
            .iter()
            .map(|&s| (s, if s == 0 { 45 } else { 10 + s }))
            .collect();
        for _ in 0..AVG_FRAMES * 2 {
            g = select(&g, &quiet_h);
        }
        if n > k {
            assert!(!g.set.contains(&0));
            assert_eq!(
                copies(&g.set, &slots, &slots)
                    .iter()
                    .filter(|(_, t)| *t != 0)
                    .count(),
                (n - 2) * k
            );
        }
        // Dedicated host: all N are remote.
        let ded = copies(&f.set, &slots, &slots).len();
        assert_eq!(ded, if n > k { n * k - k } else { n * (n - 1) });
        // Silence gate (G6-D1, Q4 decided yes): one speaker. The others send only level-only
        // datagrams, which are never forwarded (see GC-S06b).
        let mut gte = Fwd::new(k);
        for (speaker, down, up) in [(1u8, 1, n - 2), (0u8, 0, n - 1)] {
            for _ in 0..AVG_FRAMES * 2 {
                gte = select(&gte, &BTreeMap::from([(speaker, 20)]));
            }
            let sent = BTreeSet::from([speaker]);
            let c = copies(&gte.set, &sent, &slots);
            assert_eq!(
                c.iter().filter(|(_, t)| *t != 0).count(),
                up,
                "gated up N={n}"
            );
            assert_eq!(usize::from(speaker != 0), down);
        }
    }
    // Q3: the group bitrate. ASSUMPTION: the overhead is the same at 32 kbps. Then 150 bytes /
    // 60 kbps, about 21 % less. The spec says 145 bytes / 58 kbps / 24 %.
    assert_eq!(stream_bytes(GROUP_BITRATE_BPS), 150);
    assert_eq!(kbps(GROUP_BITRATE_BPS), 60);
    assert_eq!(
        (kbps(48_000) - kbps(GROUP_BITRATE_BPS)) * 100 / kbps(48_000),
        21
    );

    // Hysteresis: three speakers with close, jittery levels and K = 2.
    let run = |hyst: bool| {
        let mut f = Fwd::new(2);
        f.hysteresis = hyst;
        f.dominance_cap = false;
        let mut rng = SimRng(6);
        let mut changes = 0;
        let mut last = BTreeSet::new();
        for t in 0..500 {
            let lv: BTreeMap<u8, u8> = [(1u8, 30u8), (2, 31), (3, 32)]
                .iter()
                .map(|&(s, b)| (s, b - 6 + (rng.next_u64() % 13) as u8))
                .collect();
            f = select(&f, &lv);
            if t > AVG_FRAMES && f.set != last {
                changes += 1;
            }
            last = f.set.clone();
        }
        changes
    };
    assert_eq!(run(true), 0, "no flapping with hysteresis");
    assert!(
        run(false) >= 10,
        "flapping without hysteresis: {}",
        run(false)
    );
    // Replacement: S = {1 at 30, 2 at 32}. Slot 3 is present at 60 (not speech) and then bursts
    // at level 5 for L frames. Its average first reaches 32 - 6 = 26 on burst frame 10, so the
    // swap needs 10 qualifying frames and happens on frame 19. A 200 ms (10-frame) burst never
    // swaps.
    for (burst, swap_at) in [(10usize, None), (30, Some(19usize))] {
        let mut f = Fwd::new(2);
        f.dominance_cap = false;
        f.provisional = false;
        let steady = BTreeMap::from([(1u8, 30u8), (2, 32), (3, 60)]);
        for _ in 0..AVG_FRAMES * 2 {
            f = select(&f, &steady);
        }
        assert_eq!(f.set, BTreeSet::from([1, 2]));
        let mut at = None;
        for t in 1..=burst + AVG_FRAMES {
            let lv = BTreeMap::from([(1u8, 30u8), (2, 32), (3, if t <= burst { 5 } else { 60 })]);
            f = select(&f, &lv);
            if at.is_none() && f.set.contains(&3) {
                at = Some(t);
            }
        }
        assert_eq!(at, swap_at, "burst {burst}");
    }
    // G6-D2 (decided): the same bursts with the provisional slot. Slot 3 is forwarded from its
    // first frame, at most K+1 slots are forwarded, and after HYST_FRAMES its spurt (level 5,
    // 6 dB or more below S's weakest average) puts it in S.
    for burst in [10usize, 30] {
        let mut f = Fwd::new(2);
        f.dominance_cap = false;
        let steady = BTreeMap::from([(1u8, 30u8), (2, 32), (3, 60)]);
        for _ in 0..AVG_FRAMES * 2 {
            f = select(&f, &steady);
        }
        let (mut fwd_at, mut sel_at) = (None, None);
        for t in 1..=burst + AVG_FRAMES {
            let lv = BTreeMap::from([(1u8, 30u8), (2, 32), (3, if t <= burst { 5 } else { 60 })]);
            f = select(&f, &lv);
            assert!(f.forwarded().len() <= 3, "at most K + 1");
            if fwd_at.is_none() && f.forwarded().contains(&3) {
                fwd_at = Some(t);
            }
            if sel_at.is_none() && f.set.contains(&3) {
                sel_at = Some(t);
            }
        }
        assert_eq!(
            (fwd_at, sel_at),
            (Some(1), Some(HYST_FRAMES as usize)),
            "D2 burst {burst}"
        );
    }
    // A quieter newcomer (not 6 dB louder) is forwarded for HYST_FRAMES, then dropped, and not
    // offered again while it keeps talking.
    let mut f = Fwd::new(2);
    f.dominance_cap = false;
    let steady = BTreeMap::from([(1u8, 30u8), (2, 32), (3, 60)]);
    for _ in 0..AVG_FRAMES * 2 {
        f = select(&f, &steady);
    }
    let mut forwarded = 0;
    for _ in 0..100 {
        f = select(&f, &BTreeMap::from([(1u8, 30u8), (2, 32), (3, 40)]));
        forwarded += usize::from(f.forwarded().contains(&3));
    }
    assert_eq!(forwarded, HYST_FRAMES as usize - 1);
    assert_eq!(f.set, BTreeSet::from([1, 2]));
}
