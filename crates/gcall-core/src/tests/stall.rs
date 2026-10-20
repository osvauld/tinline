//! Fresh arrivals must not be replayed into selection frames preceding their arrival.
use super::world::*;
#[test]
fn gcall_stall_fix_preserves_exact_frame_boundary_admission() {
    let (mut s, _) = base();
    s.join(A1).unwrap();
    s.join(BOB).unwrap();
    s.now += FRAME_MS;
    s.speak(A1, 20).unwrap();
    let slot = s.hosts[&H1].slot_of(&s.did(A1)).unwrap();
    let actions = s.hosts.get_mut(&H1).unwrap().tick(s.now, &s.auth);
    s.host_actions(H1, actions);
    s.pump();
    assert!(
        s.hosts[&H1].forwarder().selected().contains(&slot),
        "arrival on the tick boundary waits an extra frame"
    );
}
fn steady() -> World {
    let (mut s, _) = base_joined(&[A1, BOB, CAROL]);
    for _ in 0..AVG_FRAMES * 2 {
        s.speak(A1, 20).unwrap();
        s.step();
    }
    s
}
#[test]
fn gcall_stall_fresh_audio_survives_the_resuming_host_tick() {
    for gap in [400, 1_000, 2_000] {
        let mut s = steady();
        let slot = s.hosts[&H1].slot_of(&s.did(A1)).unwrap();
        s.now += gap; // no engine ticks / artificial historical audio while the host is stalled
        s.speak(A1, 20).unwrap();
        let actions = s.hosts.get_mut(&H1).unwrap().tick(s.now, &s.auth);
        s.host_actions(H1, actions);
        s.pump();
        assert!(
            s.hosts[&H1].forwarder().is_forwarded(slot),
            "fresh arrival discarded after {gap}ms stall"
        );
        let before = s.down.len();
        s.speak(A1, 20).unwrap();
        assert!(
            s.down[before..].iter().any(|(_, d)| d[1] == slot),
            "next fresh packet lost after {gap}ms stall"
        );
    }
}
#[test]
fn gcall_stall_expires_old_selection_without_expiring_a_new_speaker() {
    let mut s = steady();
    let old = s.hosts[&H1].slot_of(&s.did(A1)).unwrap();
    let new = s.hosts[&H1].slot_of(&s.did(CAROL)).unwrap();
    s.now += 1_000;
    s.speak(CAROL, 20).unwrap();
    let actions = s.hosts.get_mut(&H1).unwrap().tick(s.now, &s.auth);
    s.host_actions(H1, actions);
    s.pump();
    assert!(
        !s.hosts[&H1].forwarder().is_forwarded(old),
        "stale speaker remains selected"
    );
    assert!(
        s.hosts[&H1].forwarder().is_forwarded(new),
        "new provisional speaker aged into the past"
    );
    s.now += FRAME_MS;
    let actions = s.hosts.get_mut(&H1).unwrap().tick(s.now, &s.auth);
    s.host_actions(H1, actions);
    s.pump();
    assert!(
        s.hosts[&H1].forwarder().selected().contains(&new),
        "fresh sample fills the now-free selected slot"
    );
}
#[test]
fn gcall_stall_with_only_silence_does_not_keep_old_speakers() {
    let mut s = steady();
    let old = s.hosts[&H1].slot_of(&s.did(A1)).unwrap();
    s.now += 1_000;
    s.send_up(A1, w::up_level(127).to_vec()).unwrap();
    let actions = s.hosts.get_mut(&H1).unwrap().tick(s.now, &s.auth);
    s.host_actions(H1, actions);
    s.pump();
    assert!(!s.hosts[&H1].forwarder().is_forwarded(old));
}
