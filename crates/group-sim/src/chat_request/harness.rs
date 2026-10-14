use super::*;

pub(super) fn pump(host: &mut Host, m: &mut Member, now: u64) -> Vec<Delivery> {
    let mut out = vec![];
    for relay in host.fetch(&m.did()) {
        let (d, replies) = m.receive(relay, now);
        for r in replies {
            host.submit(r, now).unwrap();
        }
        out.push(d);
    }
    out
}
pub(super) fn send(host: &mut Host, from: &mut Member, to: &str, now: u64) -> String {
    let (rid, relay) = from.request(to, now).unwrap();
    host.submit(relay, now).unwrap();
    rid
}
// Delivers `relay` directly and asserts it is dropped for `reason` with state unchanged.
pub(super) fn assert_rejected(m: &mut Member, relay: Relay, now: u64, reason: DropReason) {
    let before = m.snapshot();
    let (d, replies) = m.receive(relay, now);
    assert_eq!(d, Delivery::Dropped(reason));
    assert!(replies.is_empty());
    assert_eq!(before, m.snapshot(), "state changed on {reason:?}");
}
pub(super) fn flip(mut w: Wire, i: usize) -> Wire {
    w.ciphertext[i] ^= 1;
    w
}
