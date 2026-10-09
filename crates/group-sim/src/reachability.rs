//! Group consumers of a MOCK verified-succession adapter. Rotation task owns the actual
//! signature/path verifier. These fixtures do not prove succession cryptography or expiry policy.
use super::*;

#[path = "scheduler.rs"]
mod scheduler;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Audience {
    Group(String),
    Private,
}
#[derive(Clone, Debug)]
struct BindingEvidence {
    owner: String,
    device_id: u64,
    endpoint: String,
    audience: Audience,
    // Deterministic routing retirement time ONLY, not access-grant expiration.
    retire_at: u64,
}
#[derive(Clone)]
struct VerifiedSuccession {
    // Adapter contract: full verified ancestry since nominated root, with no omitted
    // endpoint-bearing predecessors. Only fixture code constructs this in the simulator.
    path: Vec<BindingEvidence>,
}
#[derive(Clone)]
struct Nomination {
    owner: String,
    initial_device: u64,
    group: String,
}
trait SuccessionAdapter {
    fn verified_path(&self, nomination: &Nomination) -> Option<VerifiedSuccession>;
}
struct MockRotation(VerifiedSuccession);
impl SuccessionAdapter for MockRotation {
    fn verified_path(&self, _nomination: &Nomination) -> Option<VerifiedSuccession> {
        Some(self.0.clone())
    }
}
#[derive(Default)]
struct HostDirectory {
    history: Vec<BindingEvidence>,
    current: Option<BindingEvidence>,
}
impl HostDirectory {
    fn install(&mut self, n: &Nomination, adapter: &impl SuccessionAdapter) -> bool {
        let Some(proof) = adapter.verified_path(n) else {
            return false;
        };
        let Some(first) = proof.path.first() else {
            return false;
        };
        if first.device_id != n.initial_device {
            return false;
        }
        // Group policy checks the WHOLE disclosed proof, not just the current leaf.
        if proof
            .path
            .iter()
            .any(|r| r.owner != n.owner || r.audience != Audience::Group(n.group.clone()))
        {
            return false;
        }
        // No nomination by DID equality alone; verified ancestry must start at nominated ID.
        // The rotation adapter is responsible for link ordering/fork/rollback verification.
        self.current = proof.path.last().cloned();
        self.history = proof.path;
        true
    }
    fn active(&self, now: u64) -> Option<&BindingEvidence> {
        self.current.as_ref().filter(|r| now < r.retire_at)
    }
}
#[derive(Default)]
struct Network {
    live: BTreeMap<String, u64>, // endpoint -> current receiver device ID
    queue: Vec<(String, u64, Envelope)>,
}
impl Network {
    fn connect(&mut self, directory: &HostDirectory, now: u64, envelope: Envelope) -> bool {
        let Some(binding) = directory.active(now) else {
            return false;
        };
        if self.live.get(&binding.endpoint) != Some(&binding.device_id) {
            return false;
        }
        self.queue
            .push((binding.endpoint.clone(), binding.device_id, envelope));
        true
    }
    fn deliver(&mut self, receiver: &mut Replica) -> usize {
        let mut delivered = 0;
        for (endpoint, device, envelope) in self.queue.drain(..) {
            if self.live.get(&endpoint) == Some(&device) {
                if receiver.receive(envelope) == Outcome::Accepted {
                    delivered += 1;
                }
            }
        }
        delivered
    }
}
fn nomination(owner: &Author, group: &str) -> Nomination {
    Nomination {
        owner: owner.id.did().into(),
        initial_device: 10,
        group: group.into(),
    }
}
fn evidence(n: &Nomination, device_id: u64, endpoint: &str, retire_at: u64) -> BindingEvidence {
    BindingEvidence {
        owner: n.owner.clone(),
        device_id,
        endpoint: endpoint.into(),
        audience: Audience::Group(n.group.clone()),
        retire_at,
    }
}

#[test]
fn historical_expired_bindings_are_retained_but_never_used_as_fallback_routes() {
    let (g, a, _b, _c) = signed_fixture();
    let n = nomination(&a, &g.id());
    let old = evidence(&n, 10, "old", 10);
    let new = evidence(&n, 11, "new", 20);
    let mut directory = HostDirectory::default();
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![old, new]
        })
    ));
    assert_eq!(directory.active(10).unwrap().device_id, 11);
    assert!(directory.active(20).is_none());
    assert_eq!(directory.history.len(), 2);
    // Even if an old endpoint is still listening, expiration cannot reactivate it.
    let mut network = Network::default();
    network.live.insert("old".into(), 10);
    let mut author = Author::new(4, "g");
    assert!(!network.connect(&directory, 20, author.post("no stale fallback")));
    assert!(network.queue.is_empty());
}
#[test]
fn every_group_proof_predecessor_is_checked_before_accepting_or_exporting_history() {
    let (g, a, _b, _c) = signed_fixture();
    let n = nomination(&a, &g.id());
    let good = vec![
        evidence(&n, 10, "group-old", 10),
        evidence(&n, 11, "group-new", 20),
    ];
    let mut directory = HostDirectory::default();
    assert!(directory.install(&n, &MockRotation(VerifiedSuccession { path: good.clone() })));
    for index in 0..good.len() {
        let mut bad = good.clone();
        bad[index].audience = Audience::Private;
        bad[index].endpoint = "SECRET-private-endpoint".into();
        assert!(!directory.install(&n, &MockRotation(VerifiedSuccession { path: bad })));
        assert_eq!(directory.history.len(), 2);
        assert!(
            directory
                .history
                .iter()
                .all(|r| !r.endpoint.contains("SECRET"))
        );
        assert_eq!(directory.current.as_ref().unwrap().endpoint, "group-new");
    }
    let mut wrong_group = good;
    wrong_group[0].audience = Audience::Group("another-group".into());
    assert!(!directory.install(&n, &MockRotation(VerifiedSuccession { path: wrong_group })));
}
#[test]
fn replaceable_device_ids_route_messages_without_new_group_delegation() {
    let (g, mut a, b, _c) = signed_fixture();
    let n = nomination(&b, &g.id());
    let mut directory = HostDirectory::default();
    let mut network = Network::default();
    let old = evidence(&n, 10, "host-old", 10);
    let new = evidence(&n, 11, "host-new", 30);
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![old.clone()]
        })
    ));
    network.live.insert("host-old".into(), 10);
    let mut host = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let mut recipient = Replica::from_genesis(g.clone(), &g.id()).unwrap();
    let first = a.post("queued before replacement");
    assert!(network.connect(&directory, 5, first.clone()));
    // Endpoint retired before queued delivery: do not pretend it migrated automatically.
    network.live.remove("host-old");
    network.live.insert("host-new".into(), 11);
    assert_eq!(network.deliver(&mut host), 0);
    assert!(!network.connect(&directory, 11, first.clone()));
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![old, new]
        })
    ));
    assert!(network.connect(&directory, 11, first.clone()));
    assert_eq!(network.deliver(&mut host), 1);
    let second = a.post("after replacement");
    assert!(network.connect(&directory, 12, second));
    assert_eq!(network.deliver(&mut host), 1);
    recipient.sync_from(&host);
    recipient.assert_same(&host);
    assert_eq!(host.accepted[&first.id()].signature, first.signature);
    // No permit-chain extension for routing succession: same original message/authority.
    assert_eq!(host.accepted.len(), 2);
}
#[test]
fn sibling_or_other_owner_path_does_not_inherit_device_nomination() {
    let (g, a, b, _c) = signed_fixture();
    let n = nomination(&a, &g.id());
    let mut directory = HostDirectory::default();
    let sibling = evidence(&n, 99, "sibling", 30);
    assert!(!directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![sibling]
        })
    ));
    let mut impostor = evidence(&n, 10, "other-owner", 30);
    impostor.owner = b.id.did().into();
    assert!(!directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![impostor]
        })
    ));
    assert!(directory.history.is_empty() && directory.current.is_none());
}
#[test]
fn offline_mutual_retirement_does_not_gain_magical_discovery() {
    let (g, mut a, b, _c) = signed_fixture();
    let n = nomination(&b, &g.id());
    let mut directory = HostDirectory::default();
    assert!(directory.install(
        &n,
        &MockRotation(VerifiedSuccession {
            path: vec![evidence(&n, 10, "known-old", 10)]
        })
    ));
    let mut network = Network::default();
    network.live.insert("unknown-new".into(), 11);
    assert!(!network.connect(&directory, 11, a.post("pending until rediscovery")));
    assert!(network.queue.is_empty());
}
