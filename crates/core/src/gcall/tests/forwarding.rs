//! Speaker selection, forwarding counts and `Active`.

use super::world::*;

// GC-S06. Forwarded counts equal the §4 formulas (every member sending all the time), no
// self-echo, at most K per member; hysteresis prevents flapping; and the replacement timing
// with and without G6-D2.
#[test]
fn gcall_s06_forwarded_counts_match_section_4_and_hysteresis_prevents_flapping() {
    let copies = |set: &BTreeSet<u8>, sent: &BTreeSet<u8>, rx: &BTreeSet<u8>| -> Vec<(u8, u8)> {
        set.intersection(sent).flat_map(|s| rx.iter().filter(move |r| *r != s).map(move |r| (*s, *r))).collect()
    };
    for (n, k, up) in [(3usize, 2usize, 3usize), (3, 3, 4), (5, 2, 7), (5, 3, 10), (8, 2, 13), (8, 3, 19), (12, 2, 21), (12, 3, 31)] {
        let slots: BTreeSet<u8> = (0..n as u8).collect();
        let levels: BTreeMap<u8, u8> = slots.iter().map(|&s| (s, 10 + 2 * s)).collect();
        let mut f = Forwarder::new(k);
        for _ in 0..AVG_FRAMES * 2 {
            f.frame(&levels);
        }
        assert_eq!(f.selected().len(), k.min(n));
        assert!(f.selected().contains(&0));
        let cp = copies(f.selected(), &slots, &slots);
        assert!(cp.iter().all(|(a, b)| a != b), "no self-echo");
        assert!(slots.iter().all(|r| cp.iter().filter(|(_, t)| t == r).count() <= k));
        let h_up = cp.iter().filter(|(_, t)| *t != 0).count();
        let formula = if n > k { (n - 2) * k + 1 } else { (n - 1) * (n - 1) };
        assert_eq!((h_up, h_up), (formula, up), "N={n} K={k}");
        assert_eq!(cp.len(), if n > k { n * k - k } else { n * (n - 1) }, "dedicated host N={n} K={k}");
    }
    // Hysteresis: three close, jittery speakers and K = 2 never flap; without it they do.
    let run = |hyst: bool| {
        let mut f = Forwarder::new(2);
        let mut rng = 6u64;
        let (mut changes, mut last) = (0, BTreeSet::new());
        for t in 0..500 {
            let lv: BTreeMap<u8, u8> = [(1u8, 30u8), (2, 31), (3, 32)]
                .iter()
                .map(|&(s, b)| {
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (s, b - 6 + ((rng >> 33) % 13) as u8)
                })
                .collect();
            if hyst {
                f.frame(&lv);
            } else {
                // No hysteresis: S is simply the K lowest averages this frame.
                let mut g = Forwarder::new(2);
                g.frame(&lv);
                f = g;
            }
            if t > AVG_FRAMES && *f.selected() != last {
                changes += 1;
            }
            last = f.selected().clone();
        }
        changes
    };
    assert_eq!(run(true), 0, "no flapping with hysteresis");
    assert!(run(false) >= 10, "flapping without hysteresis");
    // Replacement. S = {1 at 30, 2 at 32}; slot 3 present at 60, then bursts at level 5.
    // Without G6-D2: swap on burst frame 19 for a 30-frame burst (forwarded from the next
    // datagram), never for 10 frames. With it: forwarded at once (provisional), in S after
    // HYST_FRAMES for both.
    for (d2, burst, fwd_at, sel_at) in [(false, 10, None, None), (false, 30, Some(20), Some(19)), (true, 10, Some(1), Some(10)), (true, 30, Some(1), Some(10))] {
        let mut f = Forwarder::new(2);
        f.set_provisional(d2);
        let steady = BTreeMap::from([(1u8, 30u8), (2, 32), (3, 60)]);
        for _ in 0..AVG_FRAMES * 2 {
            f.frame(&steady);
        }
        let (mut fa, mut sa) = (None, None);
        for t in 1..=burst + AVG_FRAMES {
            let l3 = if t <= burst { 5 } else { 60 };
            f.offer(3, l3);
            if fa.is_none() && f.is_forwarded(3) {
                fa = Some(t);
            }
            f.frame(&BTreeMap::from([(1u8, 30u8), (2, 32), (3, l3)]));
            if sa.is_none() && f.selected().contains(&3) {
                sa = Some(t);
            }
            assert!(f.forwarded().len() <= 3, "at most K + 1");
        }
        assert_eq!((fa, sa), (fwd_at, sel_at), "d2={d2} burst {burst}");
    }
}

fn tone_frame(t: &mut audio::Tone) -> [i16; audio::FRAME] {
    let mut f = [0i16; audio::FRAME];
    t.next_frame(&mut f);
    f
}

// GC-S06b (G6-D1). Real engines and the real sender gate: H sends `Active` to every
// participant whenever the forwarded set changes, gens 1, 2, 3, … in order, and every client
// tracks H. Level-only datagrams reach H and are never forwarded. A gated sender stops Opus
// and its seq keeps counting. A stale Active is ignored.
#[test]
fn gcall_s06b_active_on_every_change_and_level_only_never_forwarded() {
    let (mut s, _) = base();
    let ps = [OLGA, A1, CAROL];
    for m in ps {
        s.join(m).unwrap();
    }
    let mut tones: Vec<audio::Tone> = (0..3).map(|i| audio::Tone::new(300.0 + 100.0 * i as f32, 8_000.0)).collect();
    let silence = [0i16; audio::FRAME];
    let (d0, c0) = (s.down.len(), s.down_ctl.len());
    let mut up_carol = vec![];
    for t in 0..350u32 {
        let script = [(OLGA, !(100..300).contains(&t)), (A1, (100..200).contains(&t)), (CAROL, t >= 300)];
        for (k, (m, on)) in script.into_iter().enumerate() {
            let pcm = if on { tone_frame(&mut tones[k]) } else { silence };
            let acts = s.clients.get_mut(&m).unwrap().on_mic(&pcm);
            if m == CAROL {
                up_carol.extend(acts.iter().filter_map(|a| match a {
                    ClientAction::Datagram(b) => Some(b.clone()),
                    _ => None,
                }));
            }
            s.client_actions(m, acts);
        }
        s.step();
        let (g, set) = s.hosts[&H1].active();
        for m in ps {
            assert_eq!(s.clients[&m].active(), (g, set), "client {m} at frame {t}");
        }
    }
    let (changes, _) = s.hosts[&H1].active();
    assert!(changes >= 4, "forwarded set changed {changes} times");
    for m in ps {
        let gens: Vec<u32> = s.down_ctl[c0..]
            .iter()
            .filter(|(d, _)| *d == m)
            .filter_map(|(_, msg)| match msg {
                GroupCallMsg::Active { r#gen, .. } => Some(*r#gen),
                _ => None,
            })
            .collect();
        assert_eq!(gens, (1..=changes).collect::<Vec<_>>(), "one Active per change at {m}");
    }
    assert!(s.hosts[&H1].level_only_rx() > 0);
    assert!(s.down[d0..].iter().all(|(_, b)| w::parse_down(b).is_ok()), "only full audio is forwarded");
    // CAROL: 15 Opus frames until the gate closes, level-only every 500 ms, Opus again at 300.
    let opus: Vec<u32> = up_carol.iter().filter(|b| b[0] == w::UP_AUDIO).map(|b| u32::from_be_bytes(b[1..5].try_into().unwrap())).collect();
    let gate = (300 / FRAME_MS) as u32;
    assert_eq!(opus[..gate as usize], (0..gate).collect::<Vec<_>>()[..]);
    assert_eq!(opus[gate as usize], 300, "seq kept counting while gated");
    let levels = up_carol.iter().filter(|b| b[0] == w::UP_LEVEL).count() as u32;
    assert_eq!(levels, (300 - gate) / (LEVEL_ONLY_MS / FRAME_MS) as u32);
    let c = s.clients.get_mut(&OLGA).unwrap();
    assert_eq!(c.on_control(GroupCallMsg::Active { r#gen: changes, slots: vec![0, 1, 2] }, s.now, &s.auth), Err(Refusal::Stale));
}


// G6-D2. S is full and a new sender crosses the threshold: its first datagram is forwarded at
// once in a provisional slot announced in `Active` (before the audio). At most one provisional
// slot. After HYST_FRAMES it is promoted (louder by HYST_DB) or dropped (and not offered again
// until it goes quiet).
#[test]
fn gcall_d2_new_speaker_forwarded_at_once_in_one_provisional_slot() {
    for loud in [true, false] {
        // A metered host, where S can be full (K = 2); unmetered hosts forward every speaker.
        let mut s = World::new();
        s.d[H1].cond = Cond { metered: true, ..Cond::desktop() };
        s.start_call(H1, OLGA, vec![BOB, OLGA]).unwrap();
        assert_eq!(s.hosts[&H1].k(), K_METERED);
        for m in [OLGA, A1, BOB, CAROL, HP] {
            s.join(m).unwrap();
        }
        let steady = [(OLGA, 20u8), (A1, 24)];
        for _ in 0..AVG_FRAMES * 2 {
            for (m, l) in steady {
                s.speak(m, l).unwrap();
            }
            s.step();
        }
        let e = &s.hosts[&H1];
        let sel: BTreeSet<u8> = steady.iter().map(|(m, _)| e.slot_of(&s.did(*m)).unwrap()).collect();
        assert_eq!(e.forwarder().forwarded(), sel, "S full, nothing provisional");
        let carol = e.slot_of(&s.did(CAROL)).unwrap();
        let hp = e.slot_of(&s.did(HP)).unwrap();
        // CAROL's first speech datagram: Active first, then the audio, in the same call.
        let (d0, c0) = (s.down.len(), s.down_ctl.len());
        let level = if loud { 5 } else { 40 };
        s.speak(CAROL, level).unwrap();
        s.speak(HP, 40).unwrap(); // a second newcomer in the same frame waits
        let fwd: Vec<&(usize, Vec<u8>)> = s.down[d0..].iter().filter(|(_, b)| b[1] == carol).collect();
        assert_eq!(fwd.len(), 4, "first words go to the four others at once");
        assert!(!s.down[d0..].iter().any(|(_, b)| b[1] == hp), "at most one provisional slot");
        let actives: Vec<(usize, u32, Vec<u8>)> = s.down_ctl[c0..]
            .iter()
            .filter_map(|(m, msg)| match msg {
                GroupCallMsg::Active { r#gen, slots } => Some((*m, *r#gen, slots.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(actives.len(), 5);
        assert!(actives.iter().all(|(_, _, sl)| sl.contains(&carol) && sl.len() == 3));
        for m in [OLGA, A1, BOB, HP] {
            assert!(s.clients[&m].active().1.contains(&carol), "{m} activated the slot before mixing it");
        }
        // 200 ms later hysteresis decides.
        let mut frames_fwd = 0;
        for f in 1..=HYST_FRAMES + 5 {
            s.step();
            for (m, l) in steady {
                s.speak(m, l).unwrap();
            }
            let d = s.down.len();
            s.speak(CAROL, level).unwrap();
            frames_fwd += usize::from(s.down[d..].iter().any(|(_, b)| b[1] == carol));
            let fw = s.hosts[&H1].forwarder().forwarded();
            assert!(fw.len() <= K_METERED + 1, "frame {f}: {fw:?}");
        }
        let fw = s.hosts[&H1].forwarder();
        if loud {
            assert!(fw.selected().contains(&carol), "promoted after 200 ms");
            assert_eq!(fw.selected().len(), K_METERED);
            assert_eq!(frames_fwd, HYST_FRAMES as usize + 5, "never cut");
        } else {
            assert!(!fw.is_forwarded(carol), "dropped after 200 ms");
            assert_eq!(fw.selected(), &sel);
            assert!(frames_fwd >= HYST_FRAMES as usize - 1 && frames_fwd <= HYST_FRAMES as usize, "{frames_fwd}");
            assert!(s.clients[&OLGA].active().1.iter().all(|x| *x != carol));
        }
    }
}

// Q2 (2026-10-14): an unmetered host forwards every speaker. Everyone talking at once: each
// member gets the audio of every other joined member and never its own; silent members send
// only the level byte and are never forwarded.
#[test]
fn gcall_q2_every_speaker_is_forwarded_and_silence_costs_only_the_level_byte() {
    let (mut s, _) = base();
    assert_eq!(s.hosts[&H1].k(), K_ALL);
    let talkers = [OLGA, A1, BOB, CAROL];
    for m in talkers.iter().chain([&HP]) {
        s.join(*m).unwrap();
    }
    let slot = |s: &World, m: usize| s.hosts[&H1].slot_of(&s.did(m)).unwrap();
    for _ in 0..AVG_FRAMES * 2 {
        for (i, m) in talkers.iter().enumerate() {
            s.speak(*m, 20 + 2 * i as u8).unwrap();
        }
        s.send_up(HP, w::up_level(audio::SPEECH_LEVEL + 40).to_vec()).unwrap(); // listening, silent
        s.step();
    }
    let all: BTreeSet<u8> = talkers.iter().map(|m| slot(&s, *m)).collect();
    assert_eq!(s.hosts[&H1].forwarder().forwarded(), all, "all four talkers forwarded");
    let d0 = s.down.len();
    for m in talkers {
        s.speak(m, 20).unwrap();
    }
    s.send_up(HP, w::up_level(audio::SPEECH_LEVEL + 40).to_vec()).unwrap();
    let hp = slot(&s, HP);
    for r in talkers.iter().chain([&HP]) {
        let got: BTreeSet<u8> = s.down[d0..].iter().filter(|(to, _)| to == r).map(|(_, b)| b[1]).collect();
        let want: BTreeSet<u8> = all.iter().copied().filter(|x| *x != slot(&s, *r)).collect();
        assert_eq!(got, want, "member {r} hears every other talker, not itself");
        assert!(!got.contains(&hp), "a silent member is never forwarded");
    }
}

// Q2: twelve slots all speaking fill S (K = 12), so each of the twelve hears the other eleven;
// a phone host keeps the four loudest, a host on metered data the two loudest.
#[test]
fn gcall_q2_twelve_speakers_fill_s_and_metered_keeps_two() {
    let slots: BTreeMap<u8, u8> = (0..12u8).map(|s| (s, 10 + s)).collect();
    let mut f = Forwarder::new(Cond::desktop().k());
    let mut m = Forwarder::new(Cond { metered: true, ..Cond::desktop() }.k());
    for _ in 0..AVG_FRAMES * 2 {
        f.frame(&slots);
        m.frame(&slots);
    }
    assert_eq!(f.selected().len(), 12);
    for r in 0..12u8 {
        assert_eq!(f.forwarded().iter().filter(|s| **s != r).count(), 11, "slot {r} hears eleven");
    }
    assert_eq!(m.selected(), &BTreeSet::from([0, 1]), "metered: the two loudest");
    let mut ph = Forwarder::new(Cond::phone(80, true).k());
    for _ in 0..AVG_FRAMES * 2 {
        ph.frame(&slots);
    }
    assert_eq!(ph.selected(), &BTreeSet::from([0, 1, 2, 3]), "phone: the four loudest");
    assert_eq!(Cond::desktop().k(), K_ALL);
    for c in [Cond::phone(80, false), Cond::phone(80, true)] {
        assert_eq!(c.k(), K_PHONE, "{c:?}");
    }
    for c in [Cond { metered: true, ..Cond::desktop() }, Cond { metered: true, ..Cond::phone(80, true) }] {
        assert_eq!(c.k(), K_METERED, "{c:?}");
    }
}

// Q2: off metered data mid-call, K goes back to every speaker and `Active` follows at once.
#[test]
fn gcall_q2_metered_mid_call_switches_k_both_ways() {
    let (mut s, _) = base();
    for m in [OLGA, A1, BOB, CAROL] {
        s.join(m).unwrap();
    }
    let talk = |s: &mut World| {
        for _ in 0..AVG_FRAMES * 2 {
            for (i, m) in [OLGA, A1, BOB, CAROL].iter().enumerate() {
                s.speak(*m, 20 + 4 * i as u8).unwrap();
            }
            s.step();
        }
    };
    talk(&mut s);
    assert_eq!(s.hosts[&H1].forwarder().forwarded().len(), 4);
    let c0 = s.down_ctl.len();
    let acts = s.hosts.get_mut(&H1).unwrap().set_cond(Cond { metered: true, ..Cond::desktop() }, &s.auth);
    s.host_actions(H1, acts);
    s.pump();
    assert_eq!(s.hosts[&H1].k(), K_METERED);
    assert_eq!(s.hosts[&H1].forwarder().forwarded().len(), K_METERED, "the quietest two dropped at once");
    assert!(s.down_ctl[c0..].iter().any(|(_, m)| matches!(m, GroupCallMsg::Active { slots, .. } if slots.len() == K_METERED)));
    let acts = s.hosts.get_mut(&H1).unwrap().set_cond(Cond::desktop(), &s.auth);
    s.host_actions(H1, acts);
    s.pump();
    assert_eq!(s.hosts[&H1].k(), K_ALL);
    talk(&mut s);
    assert_eq!(s.hosts[&H1].forwarder().forwarded().len(), 4, "every speaker again");
}
