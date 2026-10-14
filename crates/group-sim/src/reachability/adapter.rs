use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Audience {
    Group(String),
    Private,
}
#[derive(Clone, Debug)]
pub(super) struct BindingEvidence {
    pub(super) owner: String,
    pub(super) device_id: u64,
    pub(super) endpoint: String,
    pub(super) audience: Audience,
    // Deterministic routing retirement time ONLY, not access-grant expiration.
    pub(super) retire_at: u64,
    // Mock link field (G-H): device ID this record claims to succeed; None for the
    // nominated root. A consistency field the group checks, NOT a cryptographic link.
    pub(super) predecessor: Option<u64>,
}
#[derive(Clone)]
pub(super) struct VerifiedSuccession {
    // Adapter contract: full verified ancestry since nominated root, with no omitted
    // endpoint-bearing predecessors. Only fixture code constructs this in the simulator.
    pub(super) path: Vec<BindingEvidence>,
}
#[derive(Clone)]
pub(super) struct Nomination {
    pub(super) owner: String,
    pub(super) initial_device: u64,
    pub(super) group: String,
}
pub(super) trait SuccessionAdapter {
    fn verified_path(&self, nomination: &Nomination) -> Option<VerifiedSuccession>;
}
pub(super) struct MockRotation(pub(super) VerifiedSuccession);
impl SuccessionAdapter for MockRotation {
    fn verified_path(&self, _nomination: &Nomination) -> Option<VerifiedSuccession> {
        Some(self.0.clone())
    }
}
#[derive(Default)]
pub(super) struct HostDirectory {
    pub(super) history: Vec<BindingEvidence>,
    pub(super) current: Option<BindingEvidence>,
}
impl HostDirectory {
    pub(super) fn install(&mut self, n: &Nomination, adapter: &impl SuccessionAdapter) -> bool {
        let Some(proof) = adapter.verified_path(n) else {
            return false;
        };
        let Some(first) = proof.path.first() else {
            return false;
        };
        if first.device_id != n.initial_device {
            return false;
        }
        // Whole-path link continuity over mock fields (G-H): root has no predecessor and
        // every later record names the previous record's device. Rejects [nominated,
        // sibling]. Signatures/forks/rollback remain the rotation verifier's contract.
        if first.predecessor.is_some()
            || proof
                .path
                .windows(2)
                .any(|w| w[1].predecessor != Some(w[0].device_id))
        {
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
        // The rotation adapter is responsible for signed ordering/fork/rollback verification.
        self.current = proof.path.last().cloned();
        self.history = proof.path;
        true
    }
    pub(super) fn active(&self, now: u64) -> Option<&BindingEvidence> {
        self.current.as_ref().filter(|r| now < r.retire_at)
    }
}
// Why a route attempt failed. Distinguishes "unreachable" from proof failure (which is
// HostDirectory::install returning false) per the integration contract.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum RouteFailure {
    // No installed binding is live at `now` (none installed, or current one retired).
    NoActiveBinding,
    // Binding is live in the directory but that endpoint/device ID is not listening.
    EndpointGone,
}
#[derive(Default)]
pub(super) struct Network {
    pub(super) live: BTreeMap<String, u64>, // endpoint -> current receiver device ID
    pub(super) queue: Vec<(String, u64, Envelope)>,
}
impl Network {
    pub(super) fn connect(
        &mut self,
        directory: &HostDirectory,
        now: u64,
        envelope: Envelope,
    ) -> Result<(), RouteFailure> {
        let Some(binding) = directory.active(now) else {
            return Err(RouteFailure::NoActiveBinding);
        };
        if self.live.get(&binding.endpoint) != Some(&binding.device_id) {
            return Err(RouteFailure::EndpointGone);
        }
        self.queue
            .push((binding.endpoint.clone(), binding.device_id, envelope));
        Ok(())
    }
    pub(super) fn deliver(&mut self, receiver: &mut Replica) -> usize {
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
pub(super) fn nomination(owner: &Author, group: &str) -> Nomination {
    Nomination {
        owner: owner.id.did().into(),
        initial_device: 10,
        group: group.into(),
    }
}
pub(super) fn evidence(n: &Nomination, device_id: u64, endpoint: &str, retire_at: u64) -> BindingEvidence {
    BindingEvidence {
        owner: n.owner.clone(),
        device_id,
        endpoint: endpoint.into(),
        audience: Audience::Group(n.group.clone()),
        retire_at,
        predecessor: None,
    }
}
pub(super) fn successor(
    prev: &BindingEvidence,
    device_id: u64,
    endpoint: &str,
    retire_at: u64,
) -> BindingEvidence {
    BindingEvidence {
        device_id,
        endpoint: endpoint.into(),
        retire_at,
        predecessor: Some(prev.device_id),
        ..prev.clone()
    }
}
pub(super) struct NoVerifiedPath;
impl SuccessionAdapter for NoVerifiedPath {
    fn verified_path(&self, _nomination: &Nomination) -> Option<VerifiedSuccession> {
        None
    }
}
