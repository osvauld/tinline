//! Packet W2-B model, rewritten by R1b: VISIBLE BLOCKING on the PRIVATE audience (user decision
//! U5 revised + section 8b Q1, `docs/design/rotation-open-decisions.md` §0). Test-only candidate
//! semantics, NOT a production wire format, verifier, scheduler or policy decision.
//!
//! What U1/U2/U5 revised/U7 fix and this model encodes as given:
//! - A device announces each rotation of its PRIVATE endpoint itself, with a DID-signed note sent
//!   directly to every UNBLOCKED contact (U1). Offline contacts get it on the next contact in
//!   either direction: the device retries its outbox, or the contact dials an endpoint of the
//!   device that is still live and is answered with the current note ("catch-up").
//! - Two endpoints per device, one DID: a private one (accepted contacts only) and a group one.
//!   They rotate independently; group traffic never carries the private endpoint (U2).
//! - U7: only a group's host holds members' group endpoints; members hold only the host's. Here
//!   Alice's device is the host of the one modelled group, so members learn Alice's group
//!   endpoint and nothing else of hers.
//! - U5 revised: a block IS visible. The blocker sends the blocked DID exactly one DID-signed
//!   "blocked" notice, and every later attempt by the blocked DID is refused with that same
//!   notice (same reason). The blocked DID is still left out of later private rotation notes
//!   and its traffic is refused. Blocking never triggers a rotation. A personal block does not
//!   change group membership.
//!
//! A "rotation note" is a sim stand-in for a permit: DID-signed (owner, audience, device,
//! endpoint, monotonic seq). The real succession verifier (G-D) is NOT built here.
//!
//! Model choices that are NOT decisions (flagged where used):
//! - `Observation` is what the dialling app can tell: `NoAnswer` (offline, retired endpoint or
//!   unknown caller), `Answered`, or `Refused(notice)`. The notice carries a fixed reason string;
//!   its wording and whether it is shown at once (8b Q2/Q3) are open and not modelled.
//! - The notice is pushed through the blocker's outbox (retried until delivered) and also
//!   returned on every refused dial. Linked siblings receive the same notice inside the block
//!   sync record and refuse with the byte-identical notice; they never sign a second one.
//! - Grace window: after a rotation the previous endpoint stays live for `grace` ticks, then is
//!   retired (unreachable at the network level). The length is G-TIME/G-RETIREMENT, a parameter.
//! - Block-list sync between linked devices is a DID-signed, per-DID last-writer-wins record
//!   delivered after a configurable lag. Every linked device holds the mnemonic, so this is
//!   authentication against outsiders only, never revocation of a hostile sibling.
//! - Unblock semantics (8b Q5, incl. whether the unblocked contact is told) and recipient-bound
//!   notes are open (G-BLOCK): modelled as candidates.
//!
//! Fake `u64` ticks, deterministic fixture keys, in-memory only, real Ed25519 signatures.
use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::SigningKey;
use identity::Identity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type Key = [u8; 32];

const NOTE_DOMAIN: &[u8] = b"test-only/w2b-rotation-note/v1\0";
const BLOCK_DOMAIN: &[u8] = b"test-only/w2b-block-sync/v1\0";
const NOTICE_DOMAIN: &[u8] = b"test-only/r1b-block-notice/v1\0";
/// The one reason every refusal carries (section 8b Q1). Wording is open (8b Q3).
const REASON_BLOCKED: &str = "blocked";

// ---- fixtures copied from rotation_fork_revocation.rs @75ed838; coordinator consolidates later.

fn fixture_seed(label: &[u8], seed: u8, generation: u64) -> Key {
    Sha256::digest([label, &[seed], &generation.to_be_bytes()].concat()).into()
}
/// Deterministic fixture endpoint key. NOT a derivation scheme: receivers never compute it.
fn fixture_key(seed: u8, generation: u64) -> SigningKey {
    SigningKey::from_bytes(&fixture_seed(
        b"test-only/fixture-endpoint\0",
        seed,
        generation,
    ))
}
/// Deterministic owner identity from fixed entropy (never a real profile). Cloning it models
/// another holder of the SAME mnemonic (linked device or thief).
fn owner(seed: u8) -> Identity {
    let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&m.to_string()).unwrap()
}

// ---- end of copied fixtures.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Audience {
    Private,
    Group,
}

fn endpoint_key(device: u8, audience: Audience, seq: u64) -> Key {
    let seed = device * 2 + if audience == Audience::Group { 1 } else { 0 };
    fixture_key(seed, seq).verifying_key().to_bytes()
}

fn slot(audience: Audience, device: u8) -> String {
    format!("{audience:?}/{device}")
}

fn ser<T: Serialize>(t: &T) -> Vec<u8> {
    serde_json::to_vec(t).unwrap()
}

/// Whether `key` appears in `bytes`, raw or in its JSON array spelling.
fn leaks(bytes: &[u8], key: &Key) -> bool {
    let json = serde_json::to_vec(key).unwrap();
    bytes.windows(32).any(|w| w == key) || bytes.windows(json.len()).any(|w| w == json.as_slice())
}

fn sig64(v: &[u8]) -> [u8; 64] {
    <[u8; 64]>::try_from(v).unwrap_or([0; 64])
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NoteClaims {
    version: u8,
    owner: String,
    audience: Audience,
    device: u8,
    endpoint: Key,
    seq: u64,
    issued_at: u64,
    /// `None`: an audience-wide note (the U1 baseline). `Some(did)`: candidate G-BLOCK
    /// recipient binding (see `recipient_bound_notes_*`).
    recipient: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Note {
    claims: NoteClaims,
    signature: Vec<u8>,
}

impl Note {
    fn bytes(c: &NoteClaims) -> Vec<u8> {
        [NOTE_DOMAIN, &ser(c)].concat()
    }
    fn issue(id: &Identity, claims: NoteClaims) -> Self {
        let signature = id.sign(&Self::bytes(&claims)).to_vec();
        Self { claims, signature }
    }
}

/// The DID-signed "blocked" notice (U5 revised). It names the blocker, the blocked DID and the
/// block-record version; it carries no endpoint, device or transport key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NoticeClaims {
    owner: String,
    to: String,
    reason: String,
    version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BlockNotice {
    claims: NoticeClaims,
    signature: Vec<u8>,
}

impl BlockNotice {
    fn bytes(c: &NoticeClaims) -> Vec<u8> {
        [NOTICE_DOMAIN, &ser(c)].concat()
    }
    fn issue(id: &Identity, claims: NoticeClaims) -> Self {
        let signature = id.sign(&Self::bytes(&claims)).to_vec();
        Self { claims, signature }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Known {
    endpoint: Key,
    seq: u64,
}

/// A contact's verified knowledge of one owner: endpoints and, if blocked, the notice it holds.
/// Accepting is atomic: any error leaves it byte-for-byte unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct View {
    me: String,
    owner: String,
    /// Candidate G-BLOCK: accept only notes addressed to `me`.
    require_recipient: bool,
    known: BTreeMap<String, Known>,
    /// The verified notice from `owner`, once received. Its app shows "blocked" from this.
    blocked_by: Option<BlockNotice>,
}

impl View {
    fn accept(&mut self, note: &Note, expected: Audience) -> Result<(), &'static str> {
        let c = &note.claims;
        let key = identity::public_key_from_did(&self.owner).ok_or("owner_did")?;
        if c.owner != self.owner
            || !identity::verify(&key, &Note::bytes(c), &sig64(&note.signature))
        {
            return Err("owner_signature");
        }
        if c.version != 1 || c.audience != expected {
            return Err("audience");
        }
        match &c.recipient {
            Some(r) if r != &self.me => return Err("recipient"),
            None if self.require_recipient => return Err("recipient"),
            _ => {}
        }
        let s = slot(c.audience, c.device);
        if let Some(k) = self.known.get(&s)
            && c.seq <= k.seq
        {
            return Err("stale");
        }
        self.known.insert(
            s,
            Known {
                endpoint: c.endpoint,
                seq: c.seq,
            },
        );
        Ok(())
    }

    fn accept_notice(&mut self, n: &BlockNotice) -> Result<(), &'static str> {
        let c = &n.claims;
        let key = identity::public_key_from_did(&self.owner).ok_or("owner_did")?;
        if c.owner != self.owner
            || !identity::verify(&key, &BlockNotice::bytes(c), &sig64(&n.signature))
        {
            return Err("owner_signature");
        }
        if c.to != self.me {
            return Err("recipient");
        }
        if c.reason != REASON_BLOCKED {
            return Err("reason");
        }
        if let Some(held) = &self.blocked_by
            && c.version <= held.claims.version
        {
            return Err("stale");
        }
        self.blocked_by = Some(n.clone());
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Contact {
    did: String,
    online: bool,
    view: View,
    /// Every byte that reached this contact (network capture), accepted or not.
    wire: Vec<Vec<u8>>,
}

impl Contact {
    fn deliver(&mut self, note: &Note, expected: Audience) -> Result<(), &'static str> {
        self.wire.push(ser(note));
        self.view.accept(note, expected)
    }
    fn deliver_notice(&mut self, n: &BlockNotice) -> Result<(), &'static str> {
        self.wire.push(ser(n));
        self.view.accept_notice(n)
    }
    /// Every distinct block notice that ever reached this contact, as bytes.
    fn notices_seen(&self) -> BTreeSet<Vec<u8>> {
        self.wire
            .iter()
            .filter(|b| serde_json::from_slice::<BlockNotice>(b).is_ok())
            .cloned()
            .collect()
    }
}

/// What the dialling app can tell. An offline device, a retired endpoint and an unknown caller
/// are `NoAnswer`; a blocker refuses with its signed notice, which the caller can tell apart.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Observation {
    NoAnswer,
    Answered,
    Refused(BlockNotice),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Endpoint {
    seq: u64,
    current: Key,
    /// The previous key and the tick it is retired at (end of the grace window).
    previous: Option<(Key, u64)>,
}

impl Endpoint {
    fn new(device: u8, audience: Audience) -> Self {
        Self {
            seq: 1,
            current: endpoint_key(device, audience, 1),
            previous: None,
        }
    }
    fn live(&self, now: u64) -> Vec<Key> {
        let mut v = vec![self.current];
        if let Some((k, retire)) = self.previous
            && now < retire
        {
            v.push(k);
        }
        v
    }
    fn rotate(&mut self, device: u8, audience: Audience, now: u64, grace: u64) {
        self.previous = Some((self.current, now + grace));
        self.seq += 1;
        self.current = endpoint_key(device, audience, self.seq);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unblock {
    /// Candidate G-BLOCK (a): re-included at the next periodic rotation only.
    AtNextRotation,
    /// Candidate G-BLOCK (b): the current note is queued at once.
    Immediate,
}

/// The block-list record siblings sync. A block carries the one notice the blocking device
/// signed, so every sibling refuses with the same bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct BlockClaims {
    owner: String,
    did: String,
    blocked: bool,
    version: u64,
    notice: Option<BlockNotice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct BlockSync {
    claims: BlockClaims,
    signature: Vec<u8>,
}

impl BlockSync {
    fn bytes(c: &BlockClaims) -> Vec<u8> {
        [BLOCK_DOMAIN, &ser(c)].concat()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
enum Out {
    Note(Note),
    Notice(BlockNotice),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Device {
    owner: String,
    id: u8,
    online: bool,
    interval: u64,
    grace: u64,
    next_rotation: u64,
    bind_recipient: bool,
    private: Endpoint,
    group: Endpoint,
    contacts: BTreeSet<String>,
    /// did -> (blocked, version): per-DID last-writer-wins.
    blocks: BTreeMap<String, (bool, u64)>,
    /// did -> the notice every refusal of that DID carries.
    notices: BTreeMap<String, BlockNotice>,
    outbox: Vec<(String, Out)>,
    /// (tick, seq) of each private rotation.
    rotations: Vec<(u64, u64)>,
    /// (tick, did) of each accepted inbound connection.
    accepted: Vec<(u64, String)>,
}

impl Device {
    fn is_blocked(&self, did: &str) -> bool {
        self.blocks.get(did).is_some_and(|b| b.0)
    }
    fn would_accept(&self, from: &str) -> bool {
        self.contacts.contains(from) && !self.is_blocked(from)
    }
    fn note(&self, id: &Identity, audience: Audience, now: u64, to: Option<&str>) -> Note {
        let e = if audience == Audience::Private {
            &self.private
        } else {
            &self.group
        };
        let recipient = if self.bind_recipient {
            to.map(str::to_string)
        } else {
            None
        };
        Note::issue(
            id,
            NoteClaims {
                version: 1,
                owner: self.owner.clone(),
                audience,
                device: self.id,
                endpoint: e.current,
                seq: e.seq,
                issued_at: now,
                recipient,
            },
        )
    }
    fn tick(&mut self, id: &Identity, now: u64) {
        for e in [&mut self.private, &mut self.group] {
            if let Some((_, retire)) = e.previous
                && now >= retire
            {
                e.previous = None;
            }
        }
        if now >= self.next_rotation {
            self.private
                .rotate(self.id, Audience::Private, now, self.grace);
            self.rotations.push((now, self.private.seq));
            self.next_rotation += self.interval;
            let to: Vec<String> = self
                .contacts
                .iter()
                .filter(|c| !self.is_blocked(c))
                .cloned()
                .collect();
            for c in to {
                let n = self.note(id, Audience::Private, now, Some(&c));
                self.outbox.push((c, Out::Note(n)));
            }
        }
    }
    fn set_block(
        &mut self,
        did: &str,
        blocked: bool,
        version: u64,
        notice: Option<BlockNotice>,
    ) -> bool {
        if self.blocks.get(did).is_some_and(|b| b.1 >= version) {
            return false;
        }
        self.blocks.insert(did.to_string(), (blocked, version));
        if blocked {
            self.outbox.retain(|(to, _)| to != did);
        }
        match (blocked, notice) {
            (true, Some(n)) => {
                self.notices.insert(did.to_string(), n);
            }
            _ => {
                self.notices.remove(did);
            }
        }
        true
    }
    fn apply_sync(&mut self, s: &BlockSync) -> Result<(), &'static str> {
        let key = identity::public_key_from_did(&self.owner).ok_or("owner_did")?;
        if s.claims.owner != self.owner
            || !identity::verify(&key, &BlockSync::bytes(&s.claims), &sig64(&s.signature))
        {
            return Err("owner_signature");
        }
        if !self.set_block(
            &s.claims.did,
            s.claims.blocked,
            s.claims.version,
            s.claims.notice.clone(),
        ) {
            return Err("stale");
        }
        Ok(())
    }
    fn reachable(&self, key: &Key, now: u64) -> bool {
        self.online && (self.private.live(now).contains(key) || self.group.live(now).contains(key))
    }
    /// An authenticated inbound connection from `from` to `to`. A refusal returns the stored
    /// notice and leaves the device unchanged.
    fn inbound(
        &mut self,
        id: &Identity,
        from: &str,
        to: &Key,
        now: u64,
    ) -> (Observation, Option<Note>) {
        if !self.online || !self.private.live(now).contains(to) || !self.contacts.contains(from) {
            return (Observation::NoAnswer, None);
        }
        if self.is_blocked(from) {
            let n = self
                .notices
                .get(from)
                .cloned()
                .expect("every block record carries its notice");
            return (Observation::Refused(n), None);
        }
        self.accepted.push((now, from.to_string()));
        let catch_up = (*to != self.private.current)
            .then(|| self.note(id, Audience::Private, now, Some(from)));
        (Observation::Answered, catch_up)
    }
}

/// U7: the group host holds members' group endpoints; members hold only the host's. Alice's
/// device hosts the modelled group and sends its own group note to members.
#[derive(Clone, Debug, Default, Serialize)]
struct Host {
    members: BTreeSet<String>,
    transcript: Vec<Vec<u8>>,
}

struct Sim {
    alice: Identity,
    now: u64,
    devices: Vec<Device>,
    contacts: BTreeMap<String, Contact>,
    host: Host,
    syncs: Vec<(u64, usize, BlockSync)>,
    block_version: u64,
}

const INTERVAL: u64 = 100;
const GRACE: u64 = 10;

/// Contact DIDs are derived once (BIP39 seed stretching is slow).
fn did(seed: u8) -> String {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<BTreeMap<u8, String>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut c = cache.lock().unwrap();
    c.entry(seed)
        .or_insert_with(|| owner(seed).did().to_string())
        .clone()
}

fn bob() -> String {
    did(11)
}
fn carol() -> String {
    did(12)
}
fn dave() -> String {
    did(13)
}
fn erin() -> String {
    did(14)
}

impl Sim {
    /// Alice (owner seed 1) with one device per entry of `first_rotation`. Bob, Carol and Dave
    /// are private contacts of every device; Bob, Carol and Erin are members of one group.
    fn new(first_rotation: &[u64]) -> Self {
        let alice = owner(1);
        let a = alice.did().to_string();
        let privates = [bob(), carol(), dave()];
        let devices: Vec<Device> = first_rotation
            .iter()
            .enumerate()
            .map(|(i, &first)| {
                let id = i as u8 + 1;
                Device {
                    owner: a.clone(),
                    id,
                    online: true,
                    interval: INTERVAL,
                    grace: GRACE,
                    next_rotation: first,
                    bind_recipient: false,
                    private: Endpoint::new(id, Audience::Private),
                    group: Endpoint::new(id, Audience::Group),
                    contacts: privates.iter().cloned().collect(),
                    blocks: BTreeMap::new(),
                    notices: BTreeMap::new(),
                    outbox: Vec::new(),
                    rotations: Vec::new(),
                    accepted: Vec::new(),
                }
            })
            .collect();
        let group: BTreeSet<String> = [bob(), carol(), erin()].into_iter().collect();
        let mut contacts = BTreeMap::new();
        for c in [bob(), carol(), dave(), erin()] {
            let mut known = BTreeMap::new();
            for d in &devices {
                // Exchanged when the contact / membership was established.
                if privates.contains(&c) {
                    known.insert(
                        slot(Audience::Private, d.id),
                        Known {
                            endpoint: d.private.current,
                            seq: 1,
                        },
                    );
                }
                if group.contains(&c) {
                    known.insert(
                        slot(Audience::Group, d.id),
                        Known {
                            endpoint: d.group.current,
                            seq: 1,
                        },
                    );
                }
            }
            let view = View {
                me: c.clone(),
                owner: a.clone(),
                require_recipient: false,
                known,
                blocked_by: None,
            };
            contacts.insert(
                c.clone(),
                Contact {
                    did: c,
                    online: true,
                    view,
                    wire: Vec::new(),
                },
            );
        }
        Self {
            alice,
            now: 0,
            devices,
            contacts,
            host: Host {
                members: group,
                transcript: Vec::new(),
            },
            syncs: Vec::new(),
            block_version: 0,
        }
    }

    fn c(&self, did: &str) -> &Contact {
        &self.contacts[did]
    }

    fn step(&mut self) {
        self.now += 1;
        let now = self.now;
        let due: Vec<(usize, BlockSync)> = self
            .syncs
            .iter()
            .filter(|(t, _, _)| *t <= now)
            .map(|(_, i, s)| (*i, s.clone()))
            .collect();
        self.syncs.retain(|(t, _, _)| *t > now);
        for (i, s) in due {
            let _ = self.devices[i].apply_sync(&s);
        }
        for d in &mut self.devices {
            d.tick(&self.alice, now);
        }
        self.flush();
    }

    fn run_until(&mut self, t: u64) {
        while self.now < t {
            self.step();
        }
    }

    /// Delivers queued notes and notices to online contacts; the rest stay queued (retry).
    fn flush(&mut self) {
        for d in &mut self.devices {
            if !d.online {
                continue;
            }
            let mut keep = Vec::new();
            for (to, out) in std::mem::take(&mut d.outbox) {
                match self.contacts.get_mut(&to) {
                    Some(c) if c.online => {
                        let _ = match &out {
                            Out::Note(n) => c.deliver(n, Audience::Private),
                            Out::Notice(n) => c.deliver_notice(n),
                        };
                    }
                    _ => keep.push((to, out)),
                }
            }
            d.outbox = keep;
        }
    }

    /// Block (or unblock) on device `dev`; siblings receive the signed record after `lag` ticks.
    /// A block signs exactly one notice, queued for the blocked DID by `dev` only.
    fn set_block(&mut self, dev: usize, who: &str, blocked: bool, lag: u64, unblock: Unblock) {
        self.block_version += 1;
        let notice = blocked.then(|| {
            BlockNotice::issue(
                &self.alice,
                NoticeClaims {
                    owner: self.alice.did().to_string(),
                    to: who.to_string(),
                    reason: REASON_BLOCKED.to_string(),
                    version: self.block_version,
                },
            )
        });
        let claims = BlockClaims {
            owner: self.alice.did().to_string(),
            did: who.to_string(),
            blocked,
            version: self.block_version,
            notice: notice.clone(),
        };
        let s = BlockSync {
            signature: self.alice.sign(&BlockSync::bytes(&claims)).to_vec(),
            claims,
        };
        assert!(self.devices[dev].set_block(who, blocked, self.block_version, notice.clone()));
        if let Some(n) = notice {
            self.devices[dev]
                .outbox
                .push((who.to_string(), Out::Notice(n)));
            self.flush();
        }
        if !blocked && unblock == Unblock::Immediate {
            let n = self.devices[dev].note(&self.alice, Audience::Private, self.now, Some(who));
            self.devices[dev]
                .outbox
                .push((who.to_string(), Out::Note(n)));
            self.flush();
        }
        for i in 0..self.devices.len() {
            if i != dev {
                self.syncs.push((self.now + lag, i, s.clone()));
            }
        }
    }

    fn block(&mut self, dev: usize, who: &str, lag: u64) {
        self.set_block(dev, who, true, lag, Unblock::AtNextRotation);
    }

    /// `who` dials the private endpoint it knows for device `dev`. A catch-up note or a refusal
    /// notice in the answer is delivered to `who`; a refusal also delivers the queued notice, so
    /// it is dropped from the outbox.
    fn dial(&mut self, who: &str, dev: usize) -> Observation {
        let d_id = self.devices[dev].id;
        let Some(k) = self.contacts[who]
            .view
            .known
            .get(&slot(Audience::Private, d_id))
            .cloned()
        else {
            return Observation::NoAnswer;
        };
        let (obs, note) = self.devices[dev].inbound(&self.alice, who, &k.endpoint, self.now);
        let c = self.contacts.get_mut(who).unwrap();
        if let Some(n) = note {
            let _ = c.deliver(&n, Audience::Private);
        }
        if let Observation::Refused(n) = &obs {
            let _ = c.deliver_notice(n);
            self.devices[dev]
                .outbox
                .retain(|(to, o)| !(to == who && matches!(o, Out::Notice(_))));
        }
        obs
    }

    /// Rotates host device `dev`'s GROUP endpoint and sends the note to the group's members.
    fn rotate_group(&mut self, dev: usize) {
        let now = self.now;
        let d = &mut self.devices[dev];
        d.group.rotate(d.id, Audience::Group, now, d.grace);
        let n = d.note(&self.alice, Audience::Group, now, None);
        self.host.transcript.push(ser(&n));
        for m in self.host.members.clone() {
            if let Some(c) = self.contacts.get_mut(&m) {
                c.deliver(&n, Audience::Group).unwrap();
            }
        }
    }

    fn private_known(&self, who: &str, dev: usize) -> Known {
        self.c(who).view.known[&slot(Audience::Private, self.devices[dev].id)].clone()
    }

    /// Every private endpoint key device `dev` has had with seq >= `from_seq`.
    fn private_keys_from(&self, dev: usize, from_seq: u64) -> Vec<Key> {
        let d = &self.devices[dev];
        (from_seq..=d.private.seq)
            .map(|s| endpoint_key(d.id, Audience::Private, s))
            .collect()
    }

    fn wire_leaks(&self, who: &str, keys: &[Key]) -> bool {
        self.c(who)
            .wire
            .iter()
            .any(|b| keys.iter().any(|k| leaks(b, k)))
    }

    /// The notice device `dev` refuses `who` with.
    fn notice_for(&self, dev: usize, who: &str) -> BlockNotice {
        self.devices[dev].notices[who].clone()
    }
}

// ---------------------------------------------------------------------------------------------
// 1. The notice: exactly one, DID-signed, verifiable; the same reason on every later attempt.

// KIND: invariant
// Bob is offline when blocked: the one notice waits in the outbox, reaches him on reconnect, and
// verifies against Alice's DID independently of the view. It names bob and the fixed reason and
// carries no endpoint key of either audience. Every later dial before the rotation is refused
// with the byte-identical notice and changes neither side. An offline device answers nothing,
// which bob can tell apart from the refusal.
#[test]
fn blocked_contact_gets_one_verifiable_notice_and_the_same_reason_on_every_attempt() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(5);
    sim.contacts.get_mut(&bob()).unwrap().online = false;
    sim.block(0, &bob(), 1);
    let n = sim.notice_for(0, &bob());
    assert_eq!(sim.c(&bob()).view.blocked_by, None, "offline: not yet told");
    assert_eq!(sim.devices[0].outbox.len(), 1);
    sim.run_until(8);
    assert_eq!(sim.devices[0].outbox.len(), 1, "retried while bob is away");
    sim.contacts.get_mut(&bob()).unwrap().online = true;
    sim.run_until(9);
    assert!(sim.devices[0].outbox.is_empty());
    assert_eq!(sim.c(&bob()).view.blocked_by.as_ref(), Some(&n));

    let alice_key = identity::public_key_from_did(&sim.alice.did().to_string()).unwrap();
    assert!(identity::verify(
        &alice_key,
        &BlockNotice::bytes(&n.claims),
        &sig64(&n.signature)
    ));
    assert_eq!(
        (n.claims.to.as_str(), n.claims.reason.as_str()),
        (bob().as_str(), REASON_BLOCKED)
    );
    let d = &sim.devices[0];
    for k in [d.private.current, d.group.current] {
        assert!(!leaks(&ser(&n), &k), "the notice carries no endpoint");
    }

    for t in [10, 40, 99] {
        sim.run_until(t);
        let dev_before = ser(&sim.devices[0]);
        let bob_view = ser(&sim.c(&bob()).view);
        assert_eq!(sim.dial(&bob(), 0), Observation::Refused(n.clone()));
        assert_eq!(
            ser(&sim.devices[0]),
            dev_before,
            "a refusal leaves no trace"
        );
        assert_eq!(ser(&sim.c(&bob()).view), bob_view, "same notice, no change");
    }
    assert_eq!(sim.c(&bob()).notices_seen(), BTreeSet::from([ser(&n)]));

    sim.devices[0].online = false;
    assert_eq!(sim.dial(&bob(), 0), Observation::NoAnswer);
    assert_eq!(sim.dial(&carol(), 0), Observation::NoAnswer);
    sim.devices[0].online = true;
    assert_eq!(sim.dial(&carol(), 0), Observation::Answered, "control");
    assert_eq!(sim.c(&carol()).notices_seen(), BTreeSet::new());
}

// KIND: invariant
// Rejection: forged, wrong-owner, tampered, truncated, misaddressed, wrong-reason, older-version
// and duplicate notices all leave bob's full view byte-identical. Bob holds the notice of a
// second block (version 3) after a block (1) and an unblock (2).
#[test]
fn forged_misaddressed_tampered_and_replayed_block_notices_leave_the_view_unchanged() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(5);
    sim.block(0, &bob(), 1);
    let first = sim.notice_for(0, &bob());
    sim.set_block(0, &bob(), false, 1, Unblock::AtNextRotation);
    sim.block(0, &bob(), 1);
    let current = sim.notice_for(0, &bob());
    assert_eq!(
        (first.claims.version, current.claims.version),
        (1, 3),
        "precondition"
    );
    assert_eq!(sim.c(&bob()).view.blocked_by.as_ref(), Some(&current));

    let mallory = owner(66);
    let forged = BlockNotice::issue(&mallory, current.claims.clone());
    let mut wrong_owner_claims = current.claims.clone();
    wrong_owner_claims.owner = mallory.did().to_string();
    let wrong_owner = BlockNotice::issue(&mallory, wrong_owner_claims);
    let mut tampered = current.clone();
    tampered.claims.version = 9;
    let mut short_sig = current.clone();
    short_sig.signature.truncate(10);
    let mut to_carol = current.claims.clone();
    to_carol.to = carol();
    let misaddressed = BlockNotice::issue(&sim.alice, to_carol);
    let mut odd_reason = current.claims.clone();
    odd_reason.reason = "spam".into();
    odd_reason.version = 10;
    let wrong_reason = BlockNotice::issue(&sim.alice, odd_reason);

    let mut view = sim.c(&bob()).view.clone();
    let before = ser(&view);
    for (n, why) in [
        (&forged, "owner_signature"),
        (&wrong_owner, "owner_signature"),
        (&tampered, "owner_signature"),
        (&short_sig, "owner_signature"),
        (&misaddressed, "recipient"),
        (&wrong_reason, "reason"),
        (&first, "stale"),   // replay of the first block's notice
        (&current, "stale"), // duplicate push of the held notice
    ] {
        assert_eq!(view.accept_notice(n), Err(why));
        assert_eq!(ser(&view), before);
    }
}

// ---------------------------------------------------------------------------------------------
// 2. Exclusion from the next private rotation note.

// KIND: invariant
#[test]
fn blocked_contact_is_excluded_from_next_private_rotation_note() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(5);
    sim.block(0, &bob(), 1);
    let n = sim.notice_for(0, &bob());
    let bob_before = ser(sim.c(&bob()));
    sim.run_until(100);

    let d = &sim.devices[0];
    assert_eq!(d.private.seq, 2, "the periodic rotation happened");
    for c in [carol(), dave()] {
        assert_eq!(
            sim.private_known(&c, 0),
            Known {
                endpoint: d.private.current,
                seq: 2
            }
        );
    }
    assert_eq!(
        ser(sim.c(&bob())),
        bob_before,
        "after the notice bob received nothing at all"
    );
    assert_eq!(sim.c(&bob()).notices_seen(), BTreeSet::from([ser(&n)]));
    assert!(!sim.wire_leaks(&bob(), &sim.private_keys_from(0, 2)));
    assert!(
        sim.devices[0].outbox.is_empty(),
        "nothing is queued for bob either"
    );
}

// ---------------------------------------------------------------------------------------------
// 3. Until the rotation: network-level reach to the old endpoint, but every attempt is refused.

// KIND: invariant
// Bob keeps network-level reach to the old endpoint until rotation + grace; every dial in that
// window is refused with the same notice. Afterwards the old key is retired and bob's dial gets
// no answer at all; his app still shows the block from the notice it holds.
#[test]
fn blocked_traffic_to_old_endpoint_is_refused_with_the_notice_until_retired() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(5);
    sim.block(0, &bob(), 1);
    let n = sim.notice_for(0, &bob());
    let old = sim.private_known(&bob(), 0).endpoint;
    for t in [10, 100, 100 + GRACE - 1] {
        sim.run_until(t);
        assert!(
            sim.devices[0].reachable(&old, sim.now),
            "network level: still reachable"
        );
        assert_eq!(sim.dial(&bob(), 0), Observation::Refused(n.clone()));
    }
    sim.run_until(100 + GRACE);
    assert!(
        !sim.devices[0].reachable(&old, sim.now),
        "network level: gone"
    );
    assert_eq!(sim.dial(&bob(), 0), Observation::NoAnswer);
    assert_eq!(sim.c(&bob()).view.blocked_by.as_ref(), Some(&n));
    assert_eq!(sim.c(&bob()).notices_seen(), BTreeSet::from([ser(&n)]));
    assert_eq!(sim.dial(&carol(), 0), Observation::Answered);
}

// ---------------------------------------------------------------------------------------------
// 4. Blocking never triggers or shifts a rotation.

// KIND: invariant
#[test]
fn blocking_never_triggers_or_shifts_a_rotation() {
    let mut plain = Sim::new(&[100, 40]);
    let mut busy = Sim::new(&[100, 40]);
    // Block/unblock events at awkward moments: just before, at and after rotations.
    let events: [(u64, bool); 6] = [
        (7, true),
        (39, false),
        (40, true),
        (99, false),
        (100, true),
        (250, false),
    ];
    for &(t, blocked) in &events {
        busy.run_until(t);
        let d0 = busy.devices[0].clone();
        busy.set_block(0, &bob(), blocked, 3, Unblock::AtNextRotation);
        // The block itself touches only the block list, the notice and bob's queue.
        let d1 = &busy.devices[0];
        assert_eq!(
            (
                ser(&d1.private),
                ser(&d1.group),
                d1.next_rotation,
                &d1.rotations
            ),
            (
                ser(&d0.private),
                ser(&d0.group),
                d0.next_rotation,
                &d0.rotations
            )
        );
    }
    plain.run_until(400);
    busy.run_until(400);
    for i in 0..2 {
        assert_eq!(
            plain.devices[i].rotations, busy.devices[i].rotations,
            "device {i} schedule"
        );
        assert_eq!(
            ser(&plain.devices[i].private),
            ser(&busy.devices[i].private)
        );
        assert_eq!(
            plain.devices[i].next_rotation,
            busy.devices[i].next_rotation
        );
    }
    assert_eq!(
        busy.devices[0].rotations,
        vec![(100, 2), (200, 3), (300, 4), (400, 5)]
    );
    assert_eq!(
        busy.devices[1].rotations,
        vec![(40, 2), (140, 3), (240, 4), (340, 5)]
    );
}

// ---------------------------------------------------------------------------------------------
// 5. Block list syncs to linked devices; the race when a sibling rotates first.

// KIND: invariant
// Before the sync the sibling still answers bob (the race window below). After it the sibling
// refuses bob with the byte-identical notice device 0 signed; bob never sees a second notice.
#[test]
fn block_syncs_to_sibling_which_then_excludes_and_refuses_with_the_same_notice() {
    let mut sim = Sim::new(&[100, 100]);
    sim.run_until(5);
    sim.block(0, &bob(), 10);
    let n = sim.notice_for(0, &bob());
    sim.run_until(14);
    assert!(sim.devices[1].would_accept(&bob()), "not synced yet");
    assert_eq!(sim.dial(&bob(), 1), Observation::Answered);
    sim.run_until(15);
    assert!(!sim.devices[1].would_accept(&bob()), "synced");
    assert_eq!(sim.dial(&bob(), 1), Observation::Refused(n.clone()));
    assert_eq!(sim.c(&bob()).notices_seen(), BTreeSet::from([ser(&n)]));

    // A forged or replayed sync record changes nothing on the sibling.
    let before = ser(&sim.devices[1]);
    let mallory = owner(66);
    let claims = BlockClaims {
        owner: sim.alice.did().to_string(),
        did: bob(),
        blocked: false,
        version: 99,
        notice: None,
    };
    let forged = BlockSync {
        signature: mallory.sign(&BlockSync::bytes(&claims)).to_vec(),
        claims,
    };
    assert_eq!(sim.devices[1].apply_sync(&forged), Err("owner_signature"));
    let c = BlockClaims {
        owner: sim.alice.did().to_string(),
        did: bob(),
        blocked: false,
        version: 1,
        notice: None,
    };
    let replay = BlockSync {
        signature: sim.alice.sign(&BlockSync::bytes(&c)).to_vec(),
        claims: c,
    };
    assert_eq!(sim.devices[1].apply_sync(&replay), Err("stale"));
    assert_eq!(ser(&sim.devices[1]), before);

    sim.run_until(100);
    assert!(!sim.wire_leaks(&bob(), &sim.private_keys_from(1, 2)));
    assert!(!sim.wire_leaks(&bob(), &sim.private_keys_from(0, 2)));
    assert_eq!(sim.private_known(&carol(), 1).seq, 2);
}

/// Steps `sim` from its current tick to `until`, counting ticks at which (a) bob holds a private
/// endpoint of device `dev` issued at or after `from_seq` that is network-reachable, and (b)
/// device `dev` would accept bob's traffic.
fn measure(sim: &mut Sim, dev: usize, from_seq: u64, until: u64) -> (u64, u64) {
    let (mut exposure, mut accepting) = (0, 0);
    loop {
        let k = sim.private_known(&bob(), dev);
        if k.seq >= from_seq && sim.devices[dev].reachable(&k.endpoint, sim.now) {
            exposure += 1;
        }
        if sim.devices[dev].would_accept(&bob()) {
            accepting += 1;
        }
        if sim.now >= until {
            return (exposure, accepting);
        }
        sim.step();
    }
}

// KIND: limitation
// The sibling rotates (t=20) before the block (t=10 on device 0) reaches it (lag 30, t=40): bob
// gets the sibling's NEW private endpoint and keeps network-level reach to it until that key is
// retired (next rotation t=120 + grace 10), i.e. 110 ticks; the sibling also accepts bob's
// traffic for the 30-tick sync lag, although bob already holds the notice from device 0.
// Expected-correct under U5 revised: 0 exposure of endpoints issued after the block. No
// ordering rule is chosen here (open: G-BLOCK sync race).
#[test]
fn sibling_rotating_before_block_sync_leaks_its_new_endpoint() {
    let mut sim = Sim::new(&[100, 20]);
    sim.run_until(10);
    sim.block(0, &bob(), 30);
    let (exposure, accepting) = measure(&mut sim, 1, 2, 300);
    assert_eq!(sim.devices[1].rotations[..2], [(20, 2), (120, 3)]);
    assert_eq!(exposure, (120 + GRACE) - 20, "exposure window in ticks");
    assert_eq!(
        accepting, 30,
        "sibling accepted bob's traffic for the sync lag"
    );
    assert!(sim.c(&bob()).view.blocked_by.is_some());
    assert!(sim.wire_leaks(&bob(), &[endpoint_key(2, Audience::Private, 2)]));
    assert!(
        !sim.wire_leaks(&bob(), &sim.private_keys_from(1, 3)),
        "later rotations exclude bob"
    );

    // Control: the same schedule with a sync lag shorter than the gap to the rotation.
    let mut fast = Sim::new(&[100, 20]);
    fast.run_until(10);
    fast.block(0, &bob(), 5);
    let (exposure, accepting) = measure(&mut fast, 1, 2, 300);
    assert_eq!((exposure, accepting), (0, 5));
}

// ---------------------------------------------------------------------------------------------
// 6. Block inside a rotation's grace window.

// KIND: invariant
// Bob and Dave are offline at the rotation, so their notes wait in the outbox. Bob is blocked
// during the grace window: his queued note is replaced by the notice, and his catch-up dial to
// the still-live old endpoint is refused with that notice (which also delivers it) instead of
// answered with the new note. Dave (control) catches up by the same dial.
#[test]
fn block_during_grace_window_replaces_queued_note_with_notice_and_refuses_catch_up() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(99);
    for c in [bob(), dave()] {
        sim.contacts.get_mut(&c).unwrap().online = false;
    }
    sim.run_until(100);
    assert_eq!(
        sim.devices[0].outbox.len(),
        2,
        "bob's and dave's notes are queued"
    );
    sim.run_until(103);
    sim.block(0, &bob(), 1);
    let n = sim.notice_for(0, &bob());
    let queued: Vec<(String, bool)> = sim.devices[0]
        .outbox
        .iter()
        .map(|(to, o)| (to.clone(), matches!(o, Out::Notice(_))))
        .collect();
    assert_eq!(queued, [(dave(), false), (bob(), true)]);

    // Both come back online and dial the old endpoint, still live in the grace window, before
    // the device's next outbox retry.
    for c in [bob(), dave()] {
        sim.contacts.get_mut(&c).unwrap().online = true;
    }
    let old = sim.private_known(&bob(), 0).endpoint;
    assert!(sim.devices[0].reachable(&old, sim.now));
    let bob_known = ser(&sim.c(&bob()).view.known);
    assert_eq!(sim.dial(&bob(), 0), Observation::Refused(n.clone()));
    assert_eq!(ser(&sim.c(&bob()).view.known), bob_known);
    assert_eq!(sim.c(&bob()).view.blocked_by.as_ref(), Some(&n));
    assert_eq!(sim.dial(&dave(), 0), Observation::Answered);
    assert_eq!(
        sim.private_known(&dave(), 0).seq,
        2,
        "dave caught up by pull"
    );

    sim.run_until(100 + GRACE);
    assert!(!sim.devices[0].reachable(&old, sim.now));
    assert!(!sim.wire_leaks(&bob(), &sim.private_keys_from(0, 2)));
    assert_eq!(sim.c(&bob()).notices_seen(), BTreeSet::from([ser(&n)]));
}

// KIND: limitation
// A note already delivered before the block cannot be recalled: bob, online at the rotation
// (t=100), holds the new endpoint when blocked at t=103 and keeps network-level reach to it
// until the NEXT periodic rotation retires it (t=200 + grace), 107 ticks. His traffic is refused
// throughout. No early-rotation option is modelled because blocking must not trigger rotation.
#[test]
fn note_delivered_just_before_block_is_not_recalled() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(103);
    assert_eq!(sim.private_known(&bob(), 0).seq, 2);
    sim.block(0, &bob(), 1);
    let (exposure, accepting) = measure(&mut sim, 0, 2, 400);
    assert_eq!(exposure, (200 + GRACE) - 103);
    assert_eq!(accepting, 0);
}

// ---------------------------------------------------------------------------------------------
// 7. Unblock: two candidates. Whether the unblocked contact is told (8b Q5) is open; in both
// candidates bob still holds the old notice.

fn blocked_then_unblocked(policy: Unblock) -> (Sim, Vec<Observation>) {
    let mut sim = Sim::new(&[100]);
    sim.run_until(5);
    sim.block(0, &bob(), 1);
    sim.run_until(150);
    sim.set_block(0, &bob(), false, 1, policy);
    let mut obs = Vec::new();
    for t in [151, 199, 200] {
        sim.run_until(t);
        obs.push(sim.dial(&bob(), 0));
    }
    (sim, obs)
}

// KIND: candidate-policy G-BLOCK
// (a) Unblock re-includes bob only at the next periodic rotation: until t=200 his only key is
// the retired one, so he cannot reach the device for 50 ticks after the unblock.
#[test]
fn unblock_reincludes_at_next_rotation() {
    let (sim, obs) = blocked_then_unblocked(Unblock::AtNextRotation);
    assert_eq!(
        obs,
        [
            Observation::NoAnswer,
            Observation::NoAnswer,
            Observation::Answered
        ]
    );
    assert_eq!(sim.private_known(&bob(), 0).seq, 3, "got the t=200 note");
    assert!(!sim.wire_leaks(&bob(), &[endpoint_key(1, Audience::Private, 2)]));
    assert_eq!(
        sim.devices[0].rotations,
        vec![(100, 2), (200, 3)],
        "unblock rotates nothing"
    );
}

// KIND: candidate-policy G-BLOCK
// (b) Unblock sends the current note at once: bob reaches the device from the next tick.
#[test]
fn unblock_sends_current_endpoint_immediately() {
    let (sim, obs) = blocked_then_unblocked(Unblock::Immediate);
    assert_eq!(
        obs,
        [
            Observation::Answered,
            Observation::Answered,
            Observation::Answered
        ]
    );
    assert!(sim.wire_leaks(&bob(), &[endpoint_key(1, Audience::Private, 2)]));
    assert_eq!(sim.private_known(&bob(), 0).seq, 3);
    assert_eq!(
        sim.devices[0].rotations,
        vec![(100, 2), (200, 3)],
        "unblock rotates nothing"
    );
}

// ---------------------------------------------------------------------------------------------
// 8. Group co-membership: the host's group endpoint only, never the private one.

// KIND: invariant
// Bob is a member of the group Alice's device hosts (U7). The personal block changes no
// membership; bob keeps receiving Alice's group notes but never a later private endpoint, and
// the group path never carries any private endpoint or the block notice.
#[test]
fn blocked_group_comember_gets_group_notes_but_never_the_new_private_endpoint() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(5);
    sim.block(0, &bob(), 1);
    let n = sim.notice_for(0, &bob());
    let members_before = sim.host.members.clone();
    sim.run_until(50);
    sim.rotate_group(0);
    sim.run_until(150);
    sim.rotate_group(0);
    assert_eq!(
        sim.host.members, members_before,
        "a personal block does not touch membership"
    );
    let g = &sim.devices[0].group;
    assert_eq!(
        sim.c(&bob()).view.known[&slot(Audience::Group, 1)],
        Known {
            endpoint: g.current,
            seq: 3
        }
    );
    assert!(!sim.wire_leaks(&bob(), &sim.private_keys_from(0, 2)));
    // The group path never carries any private endpoint, old or new, nor the notice.
    let all_private = sim.private_keys_from(0, 1);
    assert!(
        !sim.host
            .transcript
            .iter()
            .any(|b| all_private.iter().any(|k| leaks(b, k)))
    );
    assert!(!sim.host.transcript.contains(&ser(&n)));
}

// KIND: invariant
// Erin shares only the group: across rotations of both endpoints she learns no private
// endpoint, and a note of one audience is never accepted as the other (state unchanged).
#[test]
fn group_only_member_never_learns_a_private_endpoint_and_audiences_do_not_cross() {
    let mut sim = Sim::new(&[30]);
    for t in [10, 60, 110] {
        sim.run_until(t);
        sim.rotate_group(0);
    }
    sim.run_until(200);
    assert!(
        !sim.c(&erin())
            .view
            .known
            .contains_key(&slot(Audience::Private, 1))
    );
    assert!(!sim.wire_leaks(&erin(), &sim.private_keys_from(0, 1)));

    let private_note = sim.devices[0].note(&sim.alice, Audience::Private, sim.now, None);
    let group_note = sim.devices[0].note(&sim.alice, Audience::Group, sim.now, None);
    let erin_before = ser(&sim.c(&erin()).view);
    let carol_before = ser(&sim.c(&carol()).view);
    let mut erin_view = sim.c(&erin()).view.clone();
    let mut carol_view = sim.c(&carol()).view.clone();
    assert_eq!(
        erin_view.accept(&private_note, Audience::Group),
        Err("audience")
    );
    assert_eq!(
        carol_view.accept(&group_note, Audience::Private),
        Err("audience")
    );
    assert_eq!(ser(&erin_view), erin_before);
    assert_eq!(ser(&carol_view), carol_before);
}

// ---------------------------------------------------------------------------------------------
// Supporting: note verification and the forwarding gap.

// KIND: invariant
#[test]
fn forged_stale_wrong_owner_and_tampered_notes_leave_the_view_unchanged() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(100);
    let good = sim.devices[0].note(&sim.alice, Audience::Private, sim.now, None);
    let mut view = sim.c(&carol()).view.clone();
    let before = ser(&view);

    let mallory = owner(66);
    let forged = Note::issue(&mallory, good.claims.clone());
    let mut wrong_owner_claims = good.claims.clone();
    wrong_owner_claims.owner = mallory.did().to_string();
    let wrong_owner = Note::issue(&mallory, wrong_owner_claims);
    let mut tampered = good.clone();
    tampered.claims.endpoint = endpoint_key(9, Audience::Private, 9);
    let mut stale_claims = good.claims.clone();
    stale_claims.seq = 1;
    stale_claims.endpoint = endpoint_key(1, Audience::Private, 1);
    let stale = Note::issue(&sim.alice, stale_claims);
    let mut short_sig = good.clone();
    short_sig.signature.truncate(10);

    for (n, why) in [
        (&forged, "owner_signature"),
        (&wrong_owner, "owner_signature"),
        (&tampered, "owner_signature"),
        (&short_sig, "owner_signature"),
        (&stale, "stale"),
        (&good, "stale"), // replay of the note carol already holds
    ] {
        assert_eq!(view.accept(n, Audience::Private), Err(why));
        assert_eq!(ser(&view), before);
    }
}

// KIND: limitation
// An unblocked contact can forward its audience-wide note to the blocked one, which verifies it
// (it is DID-signed, not addressed) and learns the new private endpoint: network-level reach
// despite exclusion. The device still refuses bob's traffic with the same notice. No mechanism
// is chosen; see the recipient-bound candidate below and the open questions.
#[test]
fn forwarded_note_hands_the_blocked_contact_the_new_endpoint() {
    let mut sim = Sim::new(&[100]);
    sim.run_until(5);
    sim.block(0, &bob(), 1);
    let notice = sim.notice_for(0, &bob());
    sim.run_until(100);
    // Carol re-sends the exact bytes she received.
    let carols: Note = serde_json::from_slice(sim.c(&carol()).wire.last().unwrap()).unwrap();
    assert_eq!(carols.claims.recipient, None, "an audience-wide note");
    sim.contacts
        .get_mut(&bob())
        .unwrap()
        .deliver(&carols, Audience::Private)
        .unwrap();
    let new = sim.devices[0].private.current;
    assert_eq!(
        sim.private_known(&bob(), 0),
        Known {
            endpoint: new,
            seq: 2
        }
    );
    assert!(
        sim.devices[0].reachable(&new, sim.now),
        "bob now has network-level reach"
    );
    let before = ser(&sim.devices[0]);
    assert_eq!(
        sim.dial(&bob(), 0),
        Observation::Refused(notice),
        "but his traffic is still refused with the same notice"
    );
    assert_eq!(ser(&sim.devices[0]), before);
}

// KIND: candidate-policy G-BLOCK
// Recipient-bound notes: each note names its recipient and a compliant client rejects a note
// addressed to someone else, so a forwarded note does not update bob's view (unchanged). The
// endpoint bytes still reach bob's wire: binding is not confidentiality, a modified client can
// read them. Hiding them from a forwarder's recipient would need encryption to the recipient,
// which still does not stop a contact from re-encrypting the endpoint for bob.
#[test]
fn recipient_bound_notes_are_rejected_when_forwarded_but_still_leak_bytes() {
    let mut sim = Sim::new(&[100]);
    sim.devices[0].bind_recipient = true;
    for c in sim.contacts.values_mut() {
        c.view.require_recipient = true;
    }
    sim.run_until(5);
    sim.block(0, &bob(), 1);
    sim.run_until(100);
    assert_eq!(
        sim.private_known(&carol(), 0).seq,
        2,
        "carol accepted her own bound note"
    );
    let carols: Note = serde_json::from_slice(sim.c(&carol()).wire.last().unwrap()).unwrap();
    assert_eq!(carols.claims.recipient.as_deref(), Some(carol().as_str()));

    let view_before = ser(&sim.c(&bob()).view);
    let bob_c = sim.contacts.get_mut(&bob()).unwrap();
    assert_eq!(bob_c.deliver(&carols, Audience::Private), Err("recipient"));
    assert_eq!(ser(&sim.c(&bob()).view), view_before);
    assert!(
        sim.wire_leaks(&bob(), &[sim.devices[0].private.current]),
        "bytes still arrived"
    );
}
