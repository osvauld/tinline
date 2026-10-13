//! Production integration tests: real group invitations, signed call-host permits,
//! real Node APIs and iroh connections. No StaticDirectory/membership fixture.
//! Current fixed-cap test characterizes a boundary; it does not approve it as future policy.
use super::*;

fn peers(tag: &str, n: usize) -> Vec<Peer> {
    (0..n)
        .map(|i| peer(&format!("capacity-{tag}-{i}"), i % FREQ.len()))
        .collect()
}

fn setup(ps: &[Peer]) -> (String, String) {
    let ms: Vec<&Peer> = ps[1..].iter().collect();
    let g = group_with(&ps[0], &ms);
    call_hosts(&ps[0], &ps[1], &ps[2], &g);
    let call = start_call(&ps[2], &g, &[&ps[0], &ps[1]]);
    for p in ps {
        eventually(WAIT, "signed call announcement", || {
            knows_call(p, &g, &call)
        });
    }
    (g, call)
}

fn join_first(ps: &mut [Peer], g: &str, call: &str, n: usize) {
    for (i, p) in ps[..n].iter().enumerate() {
        if i != 2 {
            // starter joins automatically
            p.node.group_call_join(g.into(), call.into()).unwrap();
        }
    }
    let host_did = ps[1].did.clone();
    let mut refs: Vec<&mut Peer> = ps.iter_mut().collect();
    assert!(
        drive(&mut refs, 30_000, false, |ms| ms[..n]
            .iter()
            .all(|p| joined(p, 1, &host_did, n))),
        "all {n} participants join"
    );
}

#[test]
fn capacity_twelve_authentic_members_reject_extra_without_disruption_then_recover_space() {
    let mut ps = peers("boundary", 13);
    let (g, call) = setup(&ps);
    join_first(&mut ps, &g, &call, 12);
    let host_did = ps[1].did.clone();
    let before = ps[1].node.gcall_status().unwrap().hosting.unwrap().roster;
    assert_eq!(before.len(), 12);

    // The high-level API is asynchronous; refusal may be immediate or arrive through transport.
    let _join_result = ps[12].node.group_call_join(g.clone(), call.clone());
    let mut disrupted = false;
    let mut overflow_joined = false;
    {
        let mut refs: Vec<&mut Peer> = ps.iter_mut().collect();
        drive(&mut refs, 6_000, false, |ms| {
            disrupted |= !ms[..12].iter().all(|p| joined(p, 1, &host_did, 12));
            overflow_joined |= ms[12]
                .node
                .gcall_status()
                .and_then(|s| s.client)
                .is_some_and(|c| c.state == ClientState::Joined);
            false
        });
    }
    assert!(
        !overflow_joined,
        "current fixed cap must not admit member 13"
    );
    assert!(
        !disrupted,
        "an overload refusal must not migrate/end already healthy participants"
    );
    assert_eq!(
        ps[1].node.gcall_status().unwrap().hosting.unwrap().roster,
        before,
        "rejected join leaves authoritative roster unchanged"
    );

    // Cancel the refused attempt, free one slot, then prove the eligible member can join.
    ps[12].node.group_call_leave();
    ps[11].node.group_call_leave();
    {
        let mut refs: Vec<&mut Peer> = ps.iter_mut().collect();
        assert!(
            drive(&mut refs, 10_000, false, |ms| ms[..11]
                .iter()
                .all(|p| joined(p, 1, &host_did, 11))),
            "vacated slot is announced"
        );
    }
    ps[12]
        .node
        .group_call_join(g.clone(), call.clone())
        .unwrap();
    {
        let mut refs: Vec<&mut Peer> = ps.iter_mut().collect();
        assert!(
            drive(&mut refs, 20_000, false, |ms| ms[..11]
                .iter()
                .chain(std::iter::once(&ms[12]))
                .all(|p| joined(p, 1, &host_did, 12))),
            "freed capacity accepts the next eligible member"
        );
    }
}

#[test]
fn capacity_speaker_recovers_when_previous_selected_speakers_go_silent() {
    let mut ps = peers("recovery", 5);
    ps[4].tone = audio::Tone::new(1290.0, AMP);
    let (g, call) = setup(&ps);
    join_first(&mut ps, &g, &call, 5);
    for (i, p) in ps.iter_mut().enumerate() {
        p.speaking = (1..=3).contains(&i);
    }
    {
        let mut refs: Vec<&mut Peer> = ps.iter_mut().collect();
        drive(&mut refs, 2_000, false, |_| false);
    }
    assert_eq!(ps[1].node.gcall_status().unwrap().hosting.unwrap().k, 3);
    for (i, p) in ps.iter_mut().enumerate() {
        p.speaking = i == 4;
        p.rec.clear();
    }
    {
        let mut refs: Vec<&mut Peer> = ps.iter_mut().collect();
        drive(&mut refs, 5_000, true, |_| false);
    }
    for (i, p) in ps.iter().enumerate() {
        let last = &p.rec[p.rec.len().saturating_sub(audio::RATE as usize)..];
        let amplitude = amplitude_at(last, 1290.0);
        if i == 4 {
            assert!(amplitude < 300.0, "speaker hears its own tone: {amplitude}");
        } else {
            assert!(
                amplitude > 1000.0,
                "listener {i} never recovers new speaker: {amplitude}"
            );
        }
    }
    let roster = ps[1].node.gcall_status().unwrap().hosting.unwrap().roster;
    assert_eq!(roster.len(), 5, "speaker changes never change membership");
}
