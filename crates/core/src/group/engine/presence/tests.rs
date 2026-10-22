//! The receiver's bookkeeping of presence frames (`St::take`, `St::prune`): replay, clock skew
//! and bounded memory (review findings 5 and 6). Synthetic identities, fixed clocks.

use super::*;

fn frame(host: &identity::Identity, gid: &str, seq: u64, at: u64) -> (Presence, SignedPresence) {
    let p = Presence {
        group: gid.to_string(),
        host: host.did().to_string(),
        dev: [1; 32],
        online: vec![(host.did().to_string(), at)],
        seq,
        at,
    };
    let s = proto::Signed::sign(&p, host).unwrap();
    (p, s)
}

fn hosts(gid: &str, host: &identity::Identity) -> HashMap<String, HashSet<(String, [u8; 32])>> {
    HashMap::from([(
        gid.to_string(),
        HashSet::from([(host.did().to_string(), [1; 32])]),
    )])
}

// KIND: unit (public groups, review finding 5) — a frame is taken once and only if newer than
// any taken from that host device; once its frame has expired the old frame still cannot be
// replayed (the `seq` high-water mark stays); frames whose `at` is more than 2 minutes off our
// clock are dropped.
#[test]
fn presence_frames_cannot_be_replayed() {
    let h = identity::generate().0;
    let g = "g";
    let window = 60_000;
    let t0 = 10_000_000;
    let mut st = St::default();
    let (p1, s1) = frame(&h, g, 5, t0);
    assert!(st.take(p1.clone(), s1.clone(), t0));
    assert!(
        !st.take(p1.clone(), s1.clone(), t0 + 1),
        "the same frame again: dropped"
    );
    // The window passes: the held frame is gone, its seq is not.
    st.prune(&hosts(g, &h), t0 + window + 1, window);
    assert!(st.frames.get(g).is_none_or(|m| m.is_empty()));
    assert!(
        !st.take(p1, s1, t0 + 60_000),
        "a replay after expiry: dropped"
    );
    let (p2, s2) = frame(&h, g, 6, t0 + 60_000);
    assert!(st.take(p2, s2, t0 + 60_000), "a newer one is taken");
    // Clock skew: too old or too far ahead.
    let now = t0 + 200_000;
    let (old, so) = frame(&h, g, 7, now - PRESENCE_MAX_SKEW_MS - 1);
    assert!(!st.take(old, so, now));
    let (ahead, sa) = frame(&h, g, 8, now + PRESENCE_MAX_SKEW_MS + 1);
    assert!(!st.take(ahead, sa, now));
    let (ok, sk) = frame(&h, g, 9, now - PRESENCE_MAX_SKEW_MS + 1_000);
    assert!(st.take(ok, sk, now));
}

// KIND: unit (public groups, review findings 5 and 6) — memory stays bounded: the high-water
// marks of a device that no longer hosts, and everything of a group that is no longer open, are
// forgotten; a closed group's `notified` entry stays only until its empty list was reported.
#[test]
fn presence_state_is_pruned() {
    let h = identity::generate().0;
    let (g, other) = ("g", "other");
    let t0 = 10_000_000;
    let mut st = St::default();
    let (p, s) = frame(&h, g, 1, t0);
    assert!(st.take(p, s, t0));
    let (p, s) = frame(&h, other, 1, t0);
    assert!(st.take(p, s, t0));
    st.notified
        .insert(other.into(), BTreeSet::from([h.did().to_string()]));
    // g open but h no longer hosts it; `other` no longer open.
    let open = HashMap::from([(g.to_string(), HashSet::new())]);
    st.prune(&open, t0 + 1, 60_000);
    assert!(
        st.high.is_empty(),
        "no mark of a non-host or a closed group"
    );
    assert!(!st.frames.contains_key(other));
    assert!(
        st.notified.contains_key(other),
        "kept until the empty list is reported"
    );
    st.notified.insert(other.into(), BTreeSet::new());
    st.prune(&open, t0 + 2, 60_000);
    assert!(!st.notified.contains_key(other));
}

// KIND: unit (public groups, review finding 6) — a full frame (500 members, each DID as long as a
// frame allows, with the largest times) signs within `Presence::MAX_BODY`; a longer DID is
// refused by the frame's shape check.
#[test]
fn a_full_presence_frame_fits() {
    let h = identity::generate().0;
    let did = "d".repeat(crate::group::record::MAX_PRESENCE_DID);
    let p = Presence {
        group: "0a".repeat(32),
        host: h.did().to_string(),
        dev: [1; 32],
        online: (0..MAX_PRESENCE).map(|_| (did.clone(), u64::MAX)).collect(),
        seq: u64::MAX,
        at: u64::MAX,
    };
    let s = proto::Signed::sign(&p, &h).expect("fits");
    assert!(s.to_bytes().len() <= MAX_PRESENCE_BLOB);
    let mut long = p.clone();
    long.online[0].0.push('x');
    assert!(proto::Signed::sign(&long, &h).is_err());
    // Real DIDs are well inside the bound.
    assert!(h.did().len() <= crate::group::record::MAX_PRESENCE_DID);
}
