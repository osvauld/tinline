use super::*;

pub(super) struct Run {
    pub(super) host: Host,
    pub(super) net: Net,
    pub(super) sender: Sender,
    pub(super) sub: Replica,
    pub(super) first: Ingest,
    pub(super) durable_before: Durable,
    pub(super) view_before: View,
}
pub(super) fn run(fx: &Fx, fault: Fault, order: AckOrder) -> Run {
    let (mut host, mut net, sub) = populated(fx, order, PendingPolicy::Volatile);
    let sender = Sender::new(&[&fx.c1]);
    let durable_before = host.durable.clone();
    let view_before = view(host.replica());
    let first = host.ingest(fx.c1.clone(), fault, &mut net);
    Run {
        host,
        net,
        sender,
        sub,
        first,
        durable_before,
        view_before,
    }
}
pub(super) fn expect_injected(r: &Run, fault: Fault) {
    let hit = match fault {
        Fault::Fail(s) => r.first == Ingest::Failed(s),
        _ => r.first == Ingest::Crashed(fault),
    };
    assert!(hit, "{fault:?} not reached: {:?}", r.first);
    assert_eq!(
        r.host.volatile.is_none(),
        !matches!(fault, Fault::Fail(_)),
        "{fault:?}"
    );
}
pub(super) fn restart_if_down(host: &mut Host) {
    if host.volatile.is_none() {
        let report = host
            .open(OpenPolicy::FailClosed)
            .expect("intact store opens");
        assert!(report.is_clean());
    }
}
pub(super) fn recover(r: &mut Run) {
    restart_if_down(&mut r.host);
    deliver(&mut r.net, &mut r.sub);
    r.sender.retry(&mut r.host, &mut r.net);
    deliver(&mut r.net, &mut r.sub);
    reconcile(&r.host, &mut r.sub);
}
pub(super) fn assert_converged(r: &Run, control: &Host, fx: &Fx, fault: Fault) {
    let id = fx.c1.id();
    assert!(
        r.net.acks.iter().all(|(_, b)| *b),
        "{fault:?}: ack before commit"
    );
    for (a, _) in &r.net.acks {
        assert!(r.host.durable.originals.contains_key(a), "{fault:?}");
    }
    assert!(r.net.acked(&id), "{fault:?}: update never acknowledged");
    assert_eq!(
        canonical(&r.host.durable),
        canonical(&control.durable),
        "{fault:?}"
    );
    // Exactly one commit for the update across crash, failure and retries.
    assert_eq!(r.host.durable.writes, control.durable.writes, "{fault:?}");
    assert_eq!(view(r.host.replica()), view(control.replica()), "{fault:?}");
    assert_eq!(view(&r.sub), view(r.host.replica()), "{fault:?}");
    assert_eq!(r.sub.accepted.len(), 3, "{fault:?}");
    assert_eq!(
        r.sub.fanout.len(),
        3,
        "{fault:?}: duplicate subscriber event"
    );
    assert!(r.sender.unacked.is_empty(), "{fault:?}");
    assert_eq!(r.host.durable.originals[&id], encode(&fx.c1), "{fault:?}");
    assert_eq!(r.sub.accepted[&id].signature, fx.c1.signature, "{fault:?}");
}
