//! Timed/crash fixture composed with the signed-permit direct queue. Durable clone
//! assignments are IDEAL atomic commits, not disk/vault/power-loss tests.
use super::*;

#[derive(Clone)]
struct DurableDevice {
    view: View,
    secret: [u8; 32],
    history: Vec<Permit>,
    interval: u64,
    deadline: u64,
}

impl DurableDevice {
    fn initial(owner: &Identity, device_id: u8, now: u64, interval: u64) -> Self {
        assert!(interval > 0);
        let secret = proto::new_device_secret();
        let mut view = View::initial(owner, device_id);
        view.endpoint = proto::device_public(&secret);
        Self { view, secret, history: Vec::new(), interval, deadline: now + interval }
    }

    fn prepare(&self, owner: &Identity, now: u64) -> Option<Self> {
        if now < self.deadline { return None; }
        let mut candidate = self.clone();
        // Fixture IDs only; production replacement IDs require collision policy/randomness.
        let id = self.view.current_id[0].checked_add(10).unwrap();
        let (permit, key) = next(owner, &self.view, id, now, now + self.interval);
        candidate.view.page(std::slice::from_ref(&permit), now, &HashSet::new()).unwrap();
        candidate.secret = key.to_bytes();
        candidate.history.push(permit);
        // Candidate policy: one real successor on resume, not one fake key per missed tick.
        candidate.deadline = now + self.interval;
        Some(candidate)
    }

    fn install(&self, n: &mut Network, device: usize) {
        assert_eq!(self.view.endpoint, proto::device_public(&self.secret));
        n.peers[device].endpoint = self.view.endpoint;
    }

    fn notify(&self, n: &mut Network, device: usize, targets: &[usize]) {
        let key = SigningKey::from_bytes(&self.secret);
        for &target in targets {
            let result = n.send(device, target, &path(&self.view), &self.history, &key);
            assert!(result.is_ok() || result == Err("blocked") || result == Err("offline"));
        }
    }
}

fn restart_peer(n: &mut Network, index: usize, mut durable: Peer) {
    // Policy, routes and nonce allocation high-water survive. Issued challenges do not.
    durable.outstanding.clear();
    durable.online = true;
    n.peers[index] = durable;
}

#[test]
fn independent_configured_timers_rotate_without_blocks_and_preserve_siblings() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let mut n = Network::new();
    let mut phone = DurableDevice::initial(&alice, 1, n.now, 10);
    let mut laptop = DurableDevice::initial(&alice, 2, n.now, 17);
    let a = n.add(&alice, &[]);
    let a2 = n.add(&alice, &[]);
    let b = n.add(&bob, &[phone.view.clone(), laptop.view.clone()]);
    phone.install(&mut n, a);
    laptop.install(&mut n, a2);
    n.bootstrap();
    n.now = 11;
    assert!(phone.prepare(&alice, n.now).is_none());
    n.now = 12;
    phone = phone.prepare(&alice, n.now).unwrap(); // ideal commit before installation/notify
    phone.install(&mut n, a);
    phone.notify(&mut n, a, &[b]);
    n.deliver().unwrap();
    assert!(laptop.prepare(&alice, n.now).is_none());
    assert_eq!(n.peers[b].histories[&path(&laptop.view)].generation, 0);
    n.now = 19;
    laptop = laptop.prepare(&alice, n.now).unwrap();
    laptop.install(&mut n, a2);
    laptop.notify(&mut n, a2, &[b]);
    n.deliver().unwrap();
    assert_eq!(n.peers[b].route(&path(&phone.view), n.now), Some(phone.view.endpoint));
    assert_eq!(n.peers[b].route(&path(&laptop.view), n.now), Some(laptop.view.endpoint));
    assert_eq!((phone.deadline, laptop.deadline), (22, 36));
}

#[test]
fn crash_at_each_rotation_boundary_resumes_the_committed_key_and_generation() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    // Before commit, after commit, after transport installation, after enqueue, after ack.
    for crash_at in 0..5 {
        let mut n = Network::new();
        let mut disk = DurableDevice::initial(&alice, 1, n.now, 10);
        let a = n.add(&alice, &[]);
        let b = n.add(&bob, std::slice::from_ref(&disk.view));
        disk.install(&mut n, a);
        n.bootstrap();
        n.now = disk.deadline;
        let draft = disk.prepare(&alice, n.now).unwrap();
        if crash_at >= 1 { disk = draft.clone(); }
        if crash_at >= 2 { disk.install(&mut n, a); }
        if crash_at >= 3 { disk.notify(&mut n, a, &[b]); }
        if crash_at >= 4 { n.deliver().unwrap(); }
        // Crash drops all uncommitted owner state; old transport is not serving.
        n.peers[a].online = false;
        if crash_at == 0 {
            assert!(n.queue.is_empty());
            disk = disk.prepare(&alice, n.now).unwrap(); // unannounced draft may be replaced
        } else {
            assert_eq!(disk.view, draft.view);
            assert_eq!(disk.secret, draft.secret);
            assert!(disk.prepare(&alice, n.now).is_none());
        }
        disk.install(&mut n, a); // overdue retirement/install BEFORE online
        n.peers[a].online = true;
        disk.notify(&mut n, a, &[b]);
        while !n.queue.is_empty() { n.deliver().unwrap(); }
        assert_eq!(disk.view.generation, 1);
        assert_eq!(n.peers[b].route(&path(&disk.view), n.now), Some(disk.view.endpoint));
    }
}

#[test]
fn overdue_resume_creates_one_successor_and_keeps_block_policy_and_deadline_independent() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let (carol, _) = identity::generate();
    let mut n = Network::new();
    let mut disk = DurableDevice::initial(&alice, 1, n.now, 10);
    let a = n.add(&alice, &[]);
    let b = n.add(&bob, std::slice::from_ref(&disk.view));
    let c = n.add(&carol, std::slice::from_ref(&disk.view));
    disk.install(&mut n, a);
    n.bootstrap();
    let old = n.peers[a].endpoint;
    n.block(a, carol.did());
    assert_eq!(disk.deadline, 12);
    let saved_peer = n.peers[a].clone();
    n.peers[a].online = false;
    n.now = 47;
    disk = disk.prepare(&alice, n.now).unwrap();
    // Restoration here is offline until overdue rotation installation is finished.
    n.peers[a] = saved_peer;
    n.peers[a].online = false;
    disk.install(&mut n, a);
    n.peers[a].online = true;
    disk.notify(&mut n, a, &[b, c]);
    assert_eq!(n.queue.len(), 1);
    n.deliver().unwrap();
    assert_eq!(disk.view.generation, 1);
    assert_eq!(disk.deadline, 57);
    assert_ne!(disk.view.endpoint, old);
    assert_eq!(n.peers[c].histories[&path(&disk.view)].generation, 0);
    assert!(n.peers[a].blocked.contains(carol.did()));
}

#[test]
fn receiver_restart_preserves_routes_and_rejects_old_session_challenges() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let initial = View::initial(&alice, 1);
    let mut n = Network::new();
    let a = n.add(&alice, &[]);
    let b = n.add(&bob, std::slice::from_ref(&initial));
    n.bootstrap();
    let (permit, key) = next(&alice, &initial, 2, 1, 20);
    n.peers[a].endpoint = permit.claims.endpoint;
    n.send(a, b, &path(&initial), std::slice::from_ref(&permit), &key).unwrap();
    let old_packet = n.queue.front().unwrap().clone();
    n.deliver().unwrap();
    let saved = n.peers[b].clone(); // model commit after verified update
    restart_peer(&mut n, b, saved);
    n.queue.push_back(old_packet);
    assert_eq!(n.deliver(), Err("session_replay"));
    n.send(a, b, &path(&initial), &[permit], &key).unwrap();
    assert_eq!(n.queue.front().unwrap().session_nonce, 2);
    n.deliver().unwrap();
    assert_eq!(n.peers[b].route(&path(&initial), 2), Some(n.peers[a].endpoint));
    // Permit expiration still applies after restart; stored history is not live authority.
    n.now = 20;
    assert_eq!(n.peers[b].route(&path(&initial), n.now), None);
}

#[test]
fn receiver_crash_before_acceptance_loses_challenge_not_monotonic_nonce_allocation() {
    let (alice, _) = identity::generate();
    let initial = View::initial(&alice, 1);
    let mut n = Network::new();
    let a = n.add(&alice, &[]);
    let b = n.add(&alice, std::slice::from_ref(&initial));
    n.bootstrap();
    let (permit, key) = next(&alice, &initial, 2, 1, 20);
    n.peers[a].endpoint = permit.claims.endpoint;
    n.send(a, b, &path(&initial), std::slice::from_ref(&permit), &key).unwrap();
    let saved = n.peers[b].clone(); // nonce high-water committed; no accepted permit yet
    restart_peer(&mut n, b, saved);
    assert_eq!(n.deliver(), Err("session_replay"));
    assert_eq!(n.peers[b].histories[&path(&initial)].generation, 0);
    n.send(a, b, &path(&initial), &[permit], &key).unwrap();
    n.deliver().unwrap();
    assert_eq!(n.peers[b].histories[&path(&initial)].generation, 1);
}

#[test]
fn offline_recipient_catches_up_through_expired_history_without_reviving_old_routes() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let mut n = Network::new();
    let mut disk = DurableDevice::initial(&alice, 1, n.now, 10);
    let a = n.add(&alice, &[]);
    let b = n.add(&bob, std::slice::from_ref(&disk.view));
    disk.install(&mut n, a);
    n.bootstrap();
    n.now = 12;
    disk = disk.prepare(&alice, n.now).unwrap();
    disk.install(&mut n, a);
    disk.notify(&mut n, a, &[b]);
    n.deliver().unwrap();
    let first_endpoint = disk.view.endpoint;
    n.peers[b].online = false;
    for now in [22, 32] {
        n.now = now;
        assert_eq!(n.peers[b].route(&path(&disk.view), now), None);
        disk = disk.prepare(&alice, now).unwrap();
        disk.install(&mut n, a);
        disk.notify(&mut n, a, &[b]);
        assert!(n.queue.is_empty());
    }
    n.peers[b].online = true;
    disk.notify(&mut n, a, &[b]);
    n.deliver().unwrap();
    assert_eq!(n.peers[b].histories[&path(&disk.view)].generation, 3);
    assert_ne!(first_endpoint, disk.view.endpoint);
    assert_eq!(n.peers[b].route(&path(&disk.view), 32), Some(disk.view.endpoint));
    assert_eq!(n.peers[b].route(&path(&disk.view), 42), None);
}

#[test]
fn outstanding_challenges_are_bounded_per_receiver_and_expire_without_replay() {
    let (alice, _) = identity::generate();
    let initial = View::initial(&alice, 1);
    let mut n = Network::new();
    let a = n.add(&alice, &[]);
    let b = n.add(&alice, std::slice::from_ref(&initial));
    let c = n.add(&alice, std::slice::from_ref(&initial));
    n.bootstrap();
    let (permit, key) = next(&alice, &initial, 2, 1, 30);
    n.peers[a].endpoint = permit.claims.endpoint;
    for _ in 0..8 { n.send(a, b, &path(&initial), std::slice::from_ref(&permit), &key).unwrap(); }
    assert_eq!(n.send(a, b, &path(&initial), std::slice::from_ref(&permit), &key), Err("challenge_capacity"));
    n.send(a, c, &path(&initial), std::slice::from_ref(&permit), &key).unwrap();
    assert_eq!(n.peers[c].outstanding.len(), 1);
    n.now = 7;
    assert_eq!(n.deliver(), Err("challenge_expired"));
    n.send(a, b, &path(&initial), &[permit], &key).unwrap();
    assert_eq!(n.peers[b].outstanding.len(), 1);
    // Old queued challenges were evicted, not made valid again by fresh capacity.
    assert_eq!(n.deliver(), Err("session_replay"));
}
