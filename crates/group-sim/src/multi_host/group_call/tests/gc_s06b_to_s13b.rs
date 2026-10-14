use super::*;

// KIND: invariant
// GC-S06b (G6-D1). This runs the world forwarder with the sender silence gate. H sends
// `Active { gen, slots }` to every participant each time S changes, with gen increasing by one
// each time. Every client's active set follows H's. A stale or repeated Active is ignored with
// the client's state byte-identical. A gated sender stops Opus after GATE_FRAMES and sends
// `[0x03][level]` every LEVEL_ONLY_FRAMES. H uses those for selection and never forwards them:
// every downstream datagram is `[0x02][slot][audio]`. `seq` keeps counting while gated, so the
// first resumed packet jumps by the length of the pause.
#[test]
fn gc_s06b_active_on_every_set_change_and_level_only_datagrams_never_forwarded() {
    let (mut s, _) = base();
    let ps = [OLGA, A1, CAROL];
    for m in ps {
        s.join_shown(m).unwrap();
    }
    let f0 = s.w.frames.len();
    let mut changes = 0u32;
    let mut last = BTreeSet::new();
    for t in 0..350u32 {
        s.now += FRAME_MS;
        let script = [
            (OLGA, !(100..300).contains(&t)),
            (A1, (100..200).contains(&t)),
            (CAROL, t >= 300),
        ];
        for (m, on) in script {
            s.speak_gated(m, if on { 20 } else { 70 }).unwrap();
        }
        let set = s.host_frame(H1);
        if set != last {
            changes += 1;
            last = set.clone();
        }
        assert_eq!(s.c[H1].hosting.as_ref().unwrap().active_gen, changes);
        for m in ps {
            let j = s.c[m].joined.as_ref().unwrap();
            assert_eq!((j.active_gen, &j.active), (changes, &set), "client {m} at frame {t}");
        }
    }
    assert!(changes >= 4, "S changed {changes} times");
    let new = &s.w.frames[f0..];
    let actives: Vec<&Frame> = new.iter().filter(|f| f.kind == "active").collect();
    assert_eq!(actives.len(), changes as usize * ps.len(), "one Active per change per participant");
    for m in ps {
        let gens: Vec<u32> = actives
            .iter()
            .filter(|f| f.to == m)
            .map(|f| serde_json::from_slice::<Active>(&f.bytes).unwrap().r#gen)
            .collect();
        assert_eq!(gens, (1..=changes).collect::<Vec<_>>());
    }
    // Level-only datagrams reached H but were never forwarded.
    assert!(s.c[H1].hosting.as_ref().unwrap().level_only_rx > 0);
    assert!(new.iter().any(|f| f.kind == "dgram" && f.bytes[0] == LEVEL_ONLY));
    for f in new.iter().filter(|f| f.from == H1 && f.kind == "dgram") {
        assert_eq!(f.bytes[0], DOWN);
        assert_eq!(f.bytes.len(), 2 + 5 + 120, "forwarded datagrams are full audio only");
    }
    // Silence gate at CAROL: GATE_FRAMES Opus packets, then none until frame 300, then a jump.
    let seqs: Vec<u32> = new
        .iter()
        .filter(|f| f.from == CAROL && f.kind == "dgram" && f.bytes[0] == UP)
        .map(|f| u32::from_be_bytes(f.bytes[1..5].try_into().unwrap()))
        .collect();
    let expect: Vec<u32> = (1..=GATE_FRAMES).chain(301..=350).collect();
    assert_eq!(seqs, expect, "gated sender stops Opus and keeps counting seq");
    let level_only = new
        .iter()
        .filter(|f| f.from == CAROL && f.kind == "dgram" && f.bytes[0] == LEVEL_ONLY)
        .count() as u32;
    assert_eq!(level_only, (300 - GATE_FRAMES) / LEVEL_ONLY_FRAMES);
    // A stale or repeated Active is ignored.
    let stale = serde_json::to_vec(&Active { r#gen: changes, slots: vec![0, 1, 2] }).unwrap();
    let before = s.snap(OLGA);
    assert_eq!(s.accept_active(OLGA, &stale), Err(CR::Stale));
    assert!(before == s.snap(OLGA));
}

// KIND: limitation
// GC-S07. A sender always reports level 0. With no dominance cap it holds a slot 100 % of the
// time. The cap (§6) bounds that below DOM_SHARE_PCT, but the sender still dominates. Muting it
// locally removes it from the mix, but its forwarded slot is still used up. The host's "lower
// priority" control keeps it out of the set completely while others speak.
#[test]
fn gc_s07_level_zero_spoofer_is_bounded_by_dominance_cap_local_mute_and_lower_priority() {
    let spoofer = 9u8;
    let honest = [(1u8, 20u8), (2, 26), (3, 34)];
    let frames = 5 * 60 * 50; // 5 minutes
    let run = |cap: bool, lowered: bool| {
        let mut f = Fwd::new(K_PHONE);
        f.dominance_cap = cap;
        if lowered {
            f.lowered.insert(spoofer);
        }
        let mut held = 0usize;
        let mut honest_slots = 0usize;
        for _ in 0..frames {
            let mut lv: BTreeMap<u8, u8> = honest.iter().copied().collect();
            lv.insert(spoofer, 0);
            f = select(&f, &lv);
            held += usize::from(f.set.contains(&spoofer));
            honest_slots += f.set.iter().filter(|s| **s != spoofer).count();
        }
        (held * 100 / frames, honest_slots)
    };
    let (no_cap, honest_no_cap) = run(false, false);
    let (capped, honest_capped) = run(true, false);
    let (lowered, _) = run(true, true);
    assert_eq!(no_cap, 100, "limitation: the spoofer holds a slot permanently");
    assert!(capped < DOM_SHARE_PCT, "dominance cap bounds the share: {capped} %");
    assert!(capped >= 75, "limitation: the spoofer still dominates: {capped} %");
    assert!(honest_capped > honest_no_cap, "the cap gives honest speakers more slot time");
    assert_eq!(lowered, 0, "lower priority keeps it out while K others speak");
    // Local mute: the client drops the spoofer from its mix, but the slot is still forwarded.
    let roster: BTreeMap<u8, String> = [(1, "a"), (2, "b"), (9, "spoofer")]
        .iter()
        .map(|(s, d)| (*s, d.to_string()))
        .collect();
    let forwarded = BTreeSet::from([1u8, spoofer]);
    let muted = BTreeSet::from(["spoofer".to_string()]);
    assert_eq!(mix(&forwarded, &roster, &muted), BTreeSet::from([1]));
    assert_eq!(forwarded.len(), K_PHONE, "limitation: the muted spoofer still uses a forwarded slot");
}

// KIND: limitation (ignored: not expressible)
#[test]
#[ignore = "GC-S07: the Opus VBR packet-size plausibility check (§6 [A]) needs a real Opus encoder (crates/audio, not a group-sim dependency); measure in GC-D09"]
pub(super) fn gc_s07b_opus_packet_size_flags_claimed_loud_but_tiny_packets() {
    unimplemented!("needs real Opus frames");
}

// KIND: invariant
// GC-S08. H is lost after HEARTBEAT_MISSES missed heartbeats. (a) backups[0] is up: members
// redial it at once with host_gen+1, it signs CallStart{gen 2}, and the call continues.
// (b) backups[0] is down: members reach backups[1] after ANSWER_TIMEOUT_MS. (c) Both are down:
// "Call ended — host lost" within REDIAL_WINDOW_MS. (d) Split: some members reach b0 and others
// reach b1 at the same gen. Once the relay delivers both CallStarts, b0 wins (lower backup
// index), b1 ends superseded and its members move to b0.
#[test]
fn gc_s08_host_drop_redials_backup0_else_backup1_and_split_resolves_to_backup0() {
    let setup = || {
        let (mut s, cid) = base();
        for m in [A1, CAROL, HP] {
            s.join_shown(m).unwrap();
        }
        s.advance(2_000);
        (s, cid)
    };
    // (a)
    let (mut s, cid) = setup();
    let last_hb = s.c[A1].joined.as_ref().unwrap().last_hb_ms;
    s.w.d[H1].online = false;
    s.advance(10_000);
    let lost = last_hb + HEARTBEAT_MISSES * HEARTBEAT_MS;
    for m in [A1, CAROL, HP] {
        let r = s.redials.iter().find(|(d, _)| *d == m).map(|(_, r)| r);
        assert_eq!(r, Some(&Redial::Joined { host: BOB, at_ms: lost }), "member {m}");
        let j = s.c[m].joined.as_ref().unwrap();
        assert_eq!((j.call_id.as_str(), j.host_gen, s.dev_of(&j.host)), (cid.as_str(), 2, BOB));
    }
    assert_eq!(roster_of(&s, BOB).len(), 3);
    let (_, b) = s.c[BOB].hosting.as_ref().unwrap().start.start().unwrap();
    assert_eq!(b, &vec![s.host_ref(OLGA)], "new backups = old minus self (G9)");
    // (b)
    let (mut s, _) = setup();
    let last_hb = s.c[A1].joined.as_ref().unwrap().last_hb_ms;
    s.w.d[H1].online = false;
    s.w.d[BOB].online = false;
    s.advance(10_000);
    let lost = last_hb + HEARTBEAT_MISSES * HEARTBEAT_MS;
    for m in [A1, CAROL, HP] {
        let r = s.redials.iter().find(|(d, _)| *d == m).map(|(_, r)| r);
        assert_eq!(r, Some(&Redial::Joined { host: OLGA, at_ms: lost + ANSWER_TIMEOUT_MS }));
        assert!(ANSWER_TIMEOUT_MS <= REDIAL_WINDOW_MS);
    }
    // (c)
    let (mut s, _) = setup();
    let last_hb = s.c[A1].joined.as_ref().unwrap().last_hb_ms;
    for h in [H1, BOB, OLGA] {
        s.w.d[h].online = false;
    }
    s.advance(10_000);
    let lost = last_hb + HEARTBEAT_MISSES * HEARTBEAT_MS;
    for m in [A1, CAROL, HP] {
        let r = s.redials.iter().find(|(d, _)| *d == m).map(|(_, r)| r);
        assert_eq!(r, Some(&Redial::HostLost { at_ms: lost + REDIAL_WINDOW_MS }));
        assert!(s.c[m].joined.is_none());
        assert!(s.c[m].log.iter().any(|l| l == "Call ended — host lost"));
    }
    // (d) split
    let (mut s, cid) = setup();
    s.hold_relay = true;
    s.w.cut(HP, BOB);
    s.w.d[H1].online = false;
    s.advance(4_000);
    assert_eq!(s.c[BOB].hosting.as_ref().unwrap().host_gen, 2);
    assert_eq!(s.c[OLGA].hosting.as_ref().unwrap().host_gen, 2, "b1 started gen 2 too");
    assert_eq!(s.dev_of(&s.c[HP].joined.as_ref().unwrap().host), OLGA);
    s.w.heal(HP, BOB);
    s.flush_relay();
    assert!(s.c[OLGA].hosting.is_none(), "b1 ended superseded");
    for m in [A1, CAROL, HP] {
        let j = s.c[m].joined.as_ref().unwrap();
        assert_eq!((j.call_id.as_str(), j.host_gen, s.dev_of(&j.host)), (cid.as_str(), 2, BOB), "{m}");
    }
    assert_eq!(roster_of(&s, BOB).len(), 3);
    assert!(s.c[HP].log.iter().any(|l| l == "end:Superseded"));
}

// KIND: invariant + limitation
// GC-S09. A member is removed mid-call. An honest H ejects her at once: End{removed} goes to her
// only, everyone else gets a new Roster, and her datagrams and re-Join are refused. A rogue H
// (the `World::rogue` fixture) keeps her. (a) If the rogue lists her in the Roster, honest
// members refuse that Roster (ASSUMPTION G10), so this is detectable. (b) If the rogue leaves
// her out of the Roster but forwards her audio under another participant's slot, members cannot
// tell (limitation, §6).
#[test]
fn gc_s09_removed_member_is_ejected_by_honest_host_and_kept_by_rogue_host() {
    let all = [OLGA, A1, A2, BOB, CAROL, HP, H1, H2];
    let (mut s, cid) = base();
    for m in [OLGA, A1, BOB, CAROL] {
        s.join_shown(m).unwrap();
    }
    let epoch = s.c[OLGA].joined.as_ref().unwrap().epoch;
    s.remove_member(CAROL, &all);
    assert!(s.c[CAROL].joined.is_none());
    assert!(s.c[CAROL].log.iter().any(|l| l == "end:Removed"));
    let carol_did = s.did(CAROL);
    for m in [OLGA, A1, BOB] {
        let j = s.c[m].joined.as_ref().unwrap();
        assert_eq!(j.epoch, epoch + 1);
        assert!(!j.roster.values().any(|d| *d == carol_did));
    }
    // Her datagrams on the old connection are refused, and so is a re-Join.
    let carol_ep = s.w.gep(CAROL);
    let mut d = vec![UP];
    d.extend(audio(1, 10));
    assert_eq!(s.host_ingress(H1, &carol_ep, &d), Err(CR::NotParticipant));
    s.advance(3_000);
    let req = s.join_req(CAROL, &cid, 1, s.now);
    assert_eq!(s.host_join(H1, &carol_ep, &req), Err(CR::NotMember));

    // Rogue H.
    let (mut r, _) = base();
    r.w.rogue.insert(H1);
    for m in [OLGA, A1, BOB, CAROL] {
        r.join_shown(m).unwrap();
    }
    r.remove_member(CAROL, &all);
    assert!(r.c[CAROL].joined.is_some(), "limitation: rogue host keeps the removed member");
    assert!(r.w.d[OLGA].view.removed.contains(&r.did(CAROL)));
    // (a) The rogue's Roster still lists her, so honest members refuse it. Their state is unchanged.
    let roster = r.roster_ctl(H1, None);
    let before: Vec<Vec<u8>> = [OLGA, A1, BOB].iter().map(|&m| r.snap(m)).collect();
    for (k, m) in [OLGA, A1, BOB].into_iter().enumerate() {
        assert_eq!(r.accept_roster(m, &roster.wire()), Err(CR::NotMember));
        assert!(before[k] == r.snap(m));
    }
    // (b) The rogue leaves her out of the Roster and forwards her audio under BOB's slot.
    let clean = r.roster_ctl(H1, Some(&r.did(CAROL)));
    for m in [OLGA, A1, BOB] {
        r.accept_roster(m, &clean.wire()).unwrap();
    }
    let bob_slot = r.c[H1].hosting.as_ref().unwrap().parts[&r.did(BOB)].slot;
    let mut injected = vec![DOWN, bob_slot];
    injected.extend(audio(7, 3)); // carol's packet, relabelled
    r.client_rx(OLGA, &injected);
    let j = r.c[OLGA].joined.as_ref().unwrap();
    assert!(!j.roster.values().any(|d| *d == r.did(CAROL)), "roster looks clean");
    assert_eq!(j.mixed.get(&r.did(BOB)), Some(&1), "limitation: carol's audio is attributed to bob");
}

// KIND: invariant + limitation
// GC-S10. A personal block between two participants (BOB blocks CAROL; U5 revised, U19, U20).
// Group membership and every grant stay the same. H's state is byte-identical, and nothing
// about the block reaches H. BOB's app mutes CAROL locally ("1 blocked participant"). His audio
// still reaches her, and every other member hears both. Limitation: the §3.4 pre-join warning
// "X is in this call" cannot be shown, because non-participants never get the Roster.
#[test]
fn gc_s10_personal_block_mutes_locally_and_the_host_learns_nothing() {
    let (mut s, _) = base();
    for m in [OLGA, CAROL] {
        s.join_shown(m).unwrap();
    }
    // Pre-join: BOB has the CallStart but no Roster, so there is nothing to warn from.
    assert!(s.c[BOB].joined.is_none() && s.c[BOB].shown.is_some());
    let carol = s.did(CAROL);
    let h1_before = s.snap(H1);
    let frames_before = s.w.frames.len();
    let rosters: Vec<_> = (0..s.w.d.len()).map(|i| s.w.d[i].board.roster.clone()).collect();
    s.w.block(BOB, &carol);
    assert!(s.snap(H1) == h1_before, "H learns nothing");
    assert!(s.w.frames[frames_before..].iter().all(|f| f.to != H1 && f.from != H1));
    for (i, r) in rosters.iter().enumerate() {
        assert_eq!(&s.w.d[i].board.roster, r, "membership unchanged at {i}");
    }
    s.advance(3_000);
    s.join_shown(BOB).unwrap();
    assert!(s.c[BOB].muted.contains(&carol));
    assert!(s.c[BOB].log.iter().any(|l| l == "1 blocked participant"));
    for _ in 0..50 {
        s.frame(H1, &[(BOB, 20), (CAROL, 22)]);
    }
    let bob = s.did(BOB);
    let carol_slot = s.c[H1].hosting.as_ref().unwrap().parts[&carol].slot;
    let jb = s.c[BOB].joined.as_ref().unwrap();
    assert!(jb.heard[&carol_slot] > 0, "H still forwards carol (it does not know)");
    assert_eq!(jb.mixed.get(&carol), None, "blocker's mix excludes the blocked slot");
    assert!(s.c[CAROL].joined.as_ref().unwrap().mixed[&bob] > 0, "blocker's audio reaches the blocked person");
    let jo = s.c[OLGA].joined.as_ref().unwrap();
    assert!(jo.mixed[&bob] > 0 && jo.mixed[&carol] > 0);
    assert!(s.c[CAROL].joined.as_ref().unwrap().mixed.get(&s.did(CAROL)).is_none(), "no self-echo");
}

// KIND: invariant
// GC-S11. A U12 rotation falls due while H1 (a chat host and call host) is hosting.
// (a) The call ends before the cap: the rotation waits for the end of the call, so it is late by
// less than ROTATION_DEFER_CAP_MS. (b) The call is still going at due + cap: H1 rotates, pushes
// its new records, sends End{host_rotating, next_host: self}, and members redial its new
// endpoint at host_gen+1. The old endpoint is dead. The removed member was refused from the
// moment of removal, and she never learns the new endpoint.
#[test]
fn gc_s11_u12_rotation_is_deferred_up_to_the_cap_then_members_redial_the_new_endpoint() {
    for cap_reached in [false, true] {
        let (mut s, cid) = base();
        for m in [OLGA, A1, BOB, CAROL] {
            s.join_shown(m).unwrap();
        }
        let removed_at = s.now;
        s.remove_member(CAROL, &[OLGA, A1, A2, BOB, CAROL, HP, H1, H2]);
        s.c[H1].rotation_due_ms = Some(removed_at + ROTATION_DELAY_MS);
        assert!(s.c[CAROL].joined.is_none(), "refusal is immediate");
        let old_ep = s.w.gep(H1);
        let due = removed_at + ROTATION_DELAY_MS;
        s.now = due;
        for m in [OLGA, A1, BOB] {
            s.c[m].joined.as_mut().unwrap().last_hb_ms = due;
        }
        s.c[H1].hosting.as_mut().unwrap().parts.values_mut().for_each(|p| p.last_ms = due);
        assert_eq!(s.rotation_tick(H1), "deferred");
        if !cap_reached {
            s.advance(20 * 60_000);
            assert_eq!(s.rotation_tick(H1), "deferred");
            s.host_end(H1, EndReason::Ended, None, None);
            assert_eq!(s.rotation_tick(H1), "rotated");
            assert!(s.now - due < ROTATION_DEFER_CAP_MS);
            assert_eq!(s.w.dial(OLGA, &old_ep), Err(RouteErr::EndpointGone));
            continue;
        }
        s.advance(ROTATION_DEFER_CAP_MS);
        assert_eq!(s.rotation_tick(H1), "rotated with End{host_rotating}");
        assert_eq!(s.now - due, ROTATION_DEFER_CAP_MS);
        let new_ep = s.w.gep(H1);
        assert_ne!(old_ep, new_ep);
        assert_eq!(s.w.dial(OLGA, &old_ep), Err(RouteErr::EndpointGone));
        for m in [OLGA, A1, BOB] {
            assert!(s.c[m].log.iter().any(|l| l == "end:HostRotating"));
            let j = s.c[m].joined.as_ref().unwrap_or_else(|| panic!("{m} did not redial"));
            assert_eq!((j.call_id.as_str(), j.host_gen, j.host_ep.as_str()), (cid.as_str(), 2, new_ep.as_str()));
        }
        // The removed member has only the dead address.
        let carol_rec = &s.c[CAROL].records[&s.w.hk(H1)];
        assert_eq!(carol_rec.note.claims.endpoint, old_ep);
        assert!(!has(&s.w.received(CAROL), new_ep.as_bytes()));
    }
}

// KIND: invariant
// GC-S12. A member's periodic group-endpoint rotation during a call. Deferred: the endpoint stays
// the same while she is in the call and rotates after she leaves. Forced: she sends her note to
// the chat hosts, sends Leave, and re-Joins from the new endpoint after JOIN_MIN_GAP_MS. The call
// keeps its call_id and host_gen. A Join that still carries the old note is refused as Stale.
#[test]
fn gc_s12_member_rotation_is_deferred_or_a_clean_leave_and_rejoin() {
    let (mut s, cid) = base();
    for m in [OLGA, A1, CAROL] {
        s.join_shown(m).unwrap();
    }
    s.advance(3_000);
    // Deferred.
    s.c[A1].rotation_due_ms = Some(s.now);
    let a1_ep = s.w.gep(A1);
    let deferred = s.c[A1].joined.is_some();
    assert!(deferred);
    s.advance(5_000);
    assert_eq!(s.w.gep(A1), a1_ep, "no rotation while in the call");
    s.leave(A1).unwrap();
    s.w.rotate_member(A1);
    s.c[A1].rotation_due_ms = None;
    assert_ne!(s.w.gep(A1), a1_ep);
    // Forced, for CAROL.
    let old_note = s.w.d[CAROL].my_note.clone();
    let epoch = s.c[OLGA].joined.as_ref().unwrap().epoch;
    let reached = s.w.rotate_member(CAROL);
    assert!(reached.contains(&H1), "note reached the chat host");
    s.leave(CAROL).unwrap();
    s.advance(JOIN_MIN_GAP_MS);
    // A Join with the old note from the old endpoint is refused.
    let mut stale: JoinReq = serde_json::from_slice(&s.join_req(CAROL, &cid, 1, s.now)).unwrap();
    stale.note = old_note.clone();
    let before = s.snap_all();
    assert_eq!(
        s.host_join(H1, &old_note.claims.endpoint, &serde_json::to_vec(&stale).unwrap()),
        Err(CR::Stale)
    );
    assert_unchanged(&s, &before, "old note");
    s.join_shown(CAROL).unwrap();
    let j = s.c[CAROL].joined.as_ref().unwrap();
    assert_eq!((j.call_id.as_str(), j.host_gen), (cid.as_str(), 1));
    assert_eq!(j.my_ep, s.w.gep(CAROL));
    let jo = s.c[OLGA].joined.as_ref().unwrap();
    assert_eq!(jo.epoch, epoch + 2, "Leave + re-Join; call continues");
    assert!(jo.roster.values().any(|d| *d == s.did(CAROL)));
}

// KIND: invariant
// GC-S13. Flooding is bounded. Oversize datagrams are dropped. More than MAX_UP_PPS is cut to the
// rate plus UP_BURST. Join spam within JOIN_MIN_GAP_MS is refused with state byte-identical, and
// so is control above MAX_CONTROL. The other members are unaffected: every datagram of theirs is
// forwarded and their rosters do not change.
#[test]
fn gc_s13_floods_are_bounded_and_other_members_are_unaffected() {
    let (mut s, cid) = base();
    for m in [OLGA, A1, CAROL] {
        s.join_shown(m).unwrap();
    }
    let a1_ep = s.c[A1].joined.as_ref().unwrap().my_ep.clone();
    let mut big = vec![UP];
    big.extend(vec![0u8; MAX_DATAGRAM]);
    let olga = s.did(OLGA);
    let mut accepted = 0u64;
    let secs = 5;
    for _ in 0..secs * 50 {
        s.now += FRAME_MS;
        s.speak(OLGA, 20).unwrap();
        s.speak(A1, 25).unwrap();
        for _ in 0..4 {
            if s.speak(CAROL, 30).is_ok() {
                accepted += 1;
            }
        }
        assert_eq!(s.host_ingress(H1, &a1_ep, &big), Err(CR::Oversize));
        s.host_frame(H1);
    }
    assert!(accepted <= secs * MAX_UP_PPS + UP_BURST, "flood bounded: {accepted}");
    assert!(accepted >= secs * MAX_UP_PPS, "rate honoured: {accepted}");
    let hc = s.c[H1].hosting.as_ref().unwrap();
    assert_eq!(hc.dropped.get(&olga), None, "olga never dropped");
    let olga_slot = hc.parts[&olga].slot;
    assert_eq!(s.c[A1].joined.as_ref().unwrap().heard[&olga_slot], secs * 50, "every olga frame forwarded");
    // A level-only flood (G6-D1 `[0x03][level]`) goes through the same bucket and is never
    // forwarded. Malformed level-only lengths are dropped.
    let rx0 = s.c[H1].hosting.as_ref().unwrap().level_only_rx;
    let f0 = s.w.frames.len();
    for _ in 0..50 {
        s.now += FRAME_MS;
        for _ in 0..4 {
            let _ = s.upstream(CAROL, vec![LEVEL_ONLY, 10]);
        }
        s.host_frame(H1);
    }
    let rx = s.c[H1].hosting.as_ref().unwrap().level_only_rx - rx0;
    assert!(rx <= MAX_UP_PPS + UP_BURST, "level-only flood bounded: {rx}");
    assert!(!s.w.frames[f0..].iter().any(|f| f.from == H1 && f.kind == "dgram"));
    let carol_conn = s.c[CAROL].joined.as_ref().unwrap().my_ep.clone();
    for bad in [vec![LEVEL_ONLY], vec![LEVEL_ONLY, 1, 2]] {
        assert_eq!(s.host_ingress(H1, &carol_conn, &bad), Err(CR::Malformed));
    }
    // Join spam.
    let roster = roster_of(&s, H1);
    let carol_ep = s.w.gep(CAROL);
    let first = s.join_req(CAROL, &cid, 1, s.now + 1);
    assert!(s.host_join(H1, &carol_ep, &first).is_ok(), "a newer Join after the gap is fine");
    for _ in 0..5 {
        let r = s.join_req(CAROL, &cid, 1, s.now + 2);
        let before = s.snap_all();
        assert_eq!(s.host_join(H1, &carol_ep, &r), Err(CR::RateLimited));
        assert_unchanged(&s, &before, "join spam");
    }
    let mut huge = s.join_req(BOB, &cid, 1, s.now);
    huge.extend(vec![b' '; MAX_CONTROL]);
    let before = s.snap_all();
    assert_eq!(s.host_join(H1, &s.w.gep(BOB), &huge), Err(CR::Oversize));
    assert_unchanged(&s, &before, "oversize control");
    assert_eq!(roster_of(&s, H1), roster);
}

// KIND: finding
// GC-S13 / §6. The spec claims "a Roster with up to 12 entries fits" in MAX_CONTROL. This
// measures a 12-entry Roster in two encodings. One is this simulation's JSON. The other is the
// proto `sign_blob` shape, where each Join is a SignedBlob nested in a base64url payload,
// approximated by length only.
#[test]
fn gc_s13b_twelve_entry_roster_size_against_the_control_limit() {
    let s = Sim::new();
    let cid = "ab".repeat(16);
    let ids: Vec<Identity> = (40u8..52).map(ident).collect();
    let entries: Vec<RosterEntry> = ids
        .iter()
        .enumerate()
        .map(|(k, id)| {
            let m = CallMsg {
                v: 1,
                gid: s.w.gid.clone(),
                call_id: cid.clone(),
                signer: id.did().into(),
                host_gen: 1,
                kind: Kind::Join {
                    issued_ms: 1_760_000_000_000,
                    nonce: u64::MAX - k as u64,
                },
            };
            RosterEntry {
                slot: k as u8,
                did: id.did().into(),
                join: Ctl::sign(id, m),
            }
        })
        .collect();
    let roster = s.msg(H1, &cid, 1, Kind::Roster { epoch: 1, entries: entries.clone() });
    let sim_len = roster.wire().len();
    let b64 = |n: usize| n.div_ceil(3) * 4;
    let blob = |payload: usize| b64(payload) + b64(64) + r#"{"payload":"","signature":""}"#.len();
    let join_payload = serde_json::to_vec(&entries[0].join.msg).unwrap().len();
    let entry_len = 12 + entries[0].did.len() + blob(join_payload) + 20;
    let proto_len = blob(12 * entry_len + 200);
    println!("12-entry Roster: sim JSON {sim_len} B, proto sign_blob shape ~{proto_len} B, limit {MAX_CONTROL} B");
    assert!(sim_len > MAX_CONTROL, "finding: sim encoding does not fit");
    assert!(proto_len > MAX_CONTROL, "finding: sign_blob encoding does not fit");
}

// KIND: invariant
// GC-S14. A U7 audit over a whole call lifecycle: statuses, ranking, start, joins, media, a
// member leaving, a forced member rotation and re-Join, a removal and ejection, a U12 rotation
// with End{host_rotating}, a host drop and redial to the backup at gen 3, and the end of the call. Afterwards, no member
// device has ever received or holds another member's group endpoint (any generation) or device
// slot, except devices that hold a call-host or chat-host permit, which U7 allows. The Roster
// holds DIDs and slots only.
#[test]
fn gc_s14_u7_audit_no_member_learns_another_members_endpoint_over_the_lifecycle() {
    let (mut s, cid) = base();
    let mut eps: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    let record_eps = |s: &Sim, eps: &mut BTreeMap<usize, BTreeSet<String>>| {
        for m in MEMBERS {
            eps.entry(m).or_default().insert(s.w.gep(m));
        }
    };
    record_eps(&s, &mut eps);
    let st: Vec<Status> = CALL_HOSTS.iter().map(|&h| s.status(h, s.now, STATUS_TTL_MS)).collect();
    let _ = s.rank_at(CAROL, &st, &cid, &s.did(CAROL), 4);
    for m in [A1, CAROL, HP, A2] {
        let _ = s.join_shown(m);
        s.advance(JOIN_MIN_GAP_MS);
    }
    for _ in 0..100 {
        s.frame(H1, &[(A2, 20), (CAROL, 25), (HP, 30)]);
    }
    s.leave(HP).unwrap();
    s.w.rotate_member(A2);
    record_eps(&s, &mut eps);
    s.leave(A2).unwrap();
    s.advance(JOIN_MIN_GAP_MS);
    s.join_shown(A2).unwrap();
    s.join_shown(HP).unwrap();
    s.remove_member(CAROL, &[OLGA, A1, A2, BOB, CAROL, HP, H1, H2]);
    s.c[H1].rotation_due_ms = Some(s.now);
    s.advance(ROTATION_DEFER_CAP_MS);
    assert_eq!(s.rotation_tick(H1), "rotated with End{host_rotating}");
    record_eps(&s, &mut eps);
    s.w.d[H1].online = false;
    s.advance(5_000);
    s.w.d[H1].online = true;
    assert_eq!(s.c[BOB].hosting.as_ref().unwrap().host_gen, 3);
    for _ in 0..50 {
        s.frame(BOB, &[(A2, 20), (HP, 22)]);
    }
    s.host_end(BOB, EndReason::Ended, None, None);
    // Devices that hosted hold participants' endpoints by design (U7), so they are not audited.
    // Endpoints of devices with a call-host or chat-host permit may be known to everyone.
    assert_eq!(s.ever_hosted, BTreeSet::from([H1, BOB]));
    let permitted: BTreeSet<usize> = CALL_HOSTS.iter().copied().chain([H1, H2]).collect();
    let audited: Vec<usize> = MEMBERS.iter().copied().filter(|m| !s.ever_hosted.contains(m)).collect();
    assert_eq!(audited, vec![OLGA, A1, A2, CAROL, HP]);
    for m in audited {
        let hay = s.w.received(m);
        for (&o, set) in &eps {
            if o == m || permitted.contains(&o) || s.did(o) == s.did(m) {
                continue;
            }
            for e in set {
                assert!(!has(&hay, e.as_bytes()), "member {m} holds member {o}'s group endpoint");
            }
            let slot = s.w.d[o].slot.to_string();
            assert!(!has(&hay, format!("\"slot\":{slot}").as_bytes()), "member {m} holds member {o}'s device slot");
            assert!(!has(&hay, format!("\"device_slot\":{slot}").as_bytes()));
        }
    }
    // Every Roster frame that was sent holds DIDs and slots only.
    for f in s.w.frames.iter().filter(|f| f.kind == "roster") {
        let r: Ctl = serde_json::from_slice(&f.bytes).unwrap();
        let Kind::Roster { entries, .. } = r.msg.kind else { panic!() };
        for e in entries {
            let j = serde_json::to_string(&e).unwrap();
            assert!(!j.contains("endpoint") && !j.contains("grp-") && !j.contains("device"), "{j}");
        }
    }
}
