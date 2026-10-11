//! Packet W2-D model: PUSH THE DID-SIGNED DEVICE LIST TO CONTACTS WHEN IT CHANGES (user decision
//! U8, `docs/design/rotation-open-decisions.md` §0). Test-only candidate semantics, NOT a
//! production wire format, scheduler or policy decision.
//!
//! What U8 (with U1, U5 revised) fixes and this model encodes as given:
//! - Whenever the account's device set changes (a device added, unlinked, restored, or its
//!   private endpoint rotated) the install that made the change signs a new `DeviceList` with a
//!   higher `seq` and queues it for every UNBLOCKED contact. Only the latest list per contact is
//!   kept in the outbox; an offline contact gets it when either side next reaches the other.
//! - A blocked contact gets nothing new; instead it gets one DID-signed "blocked" notice
//!   (U5 revised: blocking is visible). Blocking does not re-sign the list or rotate anything.
//! - Today the entries of the list are device keys that are also the iroh endpoint keys
//!   (`crates/core/src/node.rs:2316`). U8 combines the list with the U1 private-endpoint note; this
//!   model uses one 32-byte key per entry for both and does not decide the split (U4 revised).
//!
//! The list itself is the REAL production code: `proto::sign_device_list`,
//! `proto::verify_device_list` and `proto::newer` (`crates/proto/src/lib.rs:713-776`). Only the
//! outbox, the contact's view, the block notice and the content check are model code.
//!
//! Model choices that are NOT decisions (flagged where used):
//! - `seq` follows production: `max(now, previous + 1)` with a fake `u64` clock in place of ms.
//! - Content ("chat messages") is signed by a device key and accepted only if that key is in the
//!   held list; a batch (history snapshot) is all-or-nothing, as `chat_keys` + snapshot
//!   verification behave today (`crates/core/src/chat/own.rs:54`, K1 §5 bug 1).
//! - Concurrent edits by two own installs (G-FORK) are modelled as three receiver candidates and
//!   one sender candidate; none is selected.
//! - Every install holds the shared mnemonic: nothing here is revocation of a hostile holder.
//!
//! Fake `u64` ticks, deterministic fixture keys, in-memory only, real Ed25519 signatures.
use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::{Signer as _, SigningKey};
use identity::Identity;
use proto::{SignedBlob, newer, sign_device_list, verify_device_list};
use serde::Serialize;
use sha2::{Digest, Sha256};

type Key = [u8; 32];

const BLOCK_DOMAIN: &[u8] = b"test-only/w2d-block-notice/v1\0";
const CONTENT_DOMAIN: &[u8] = b"test-only/w2d-content/v1\0";

// ---- fixtures copied from rotation_silent_block.rs @0924939 (originally
// rotation_fork_revocation.rs @75ed838); coordinator consolidates later.

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

fn ser<T: Serialize>(t: &T) -> Vec<u8> {
    serde_json::to_vec(t).unwrap()
}

fn pk(k: &SigningKey) -> Key {
    k.verifying_key().to_bytes()
}

/// Device `slot` at key generation `generation` (a restore or rotation bumps the generation).
fn dev(slot: u8, generation: u64) -> SigningKey {
    fixture_key(slot, generation)
}

fn did(seed: u8) -> String {
    owner(seed).did().to_string()
}

const BOB: u8 = 2;
const ALICE: u8 = 1;
const CAROL: u8 = 3;
const DAVE: u8 = 4;
const MALLORY: u8 = 9;

fn list_keys(blob: &SignedBlob, owner_did: &str) -> Vec<Key> {
    verify_device_list(blob, owner_did)
        .unwrap()
        .devices
        .iter()
        .map(|e| e.device_key().unwrap())
        .collect()
}

fn list_seq(blob: &SignedBlob, owner_did: &str) -> u64 {
    verify_device_list(blob, owner_did).unwrap().seq
}

fn set(keys: &[Key]) -> BTreeSet<Key> {
    keys.iter().copied().collect()
}

// ---- the visible block notice (U5 revised) -----------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct NoticeClaims {
    owner: String,
    blocked: String,
    issued_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct BlockNotice {
    claims: NoticeClaims,
    signature: Vec<u8>,
}

impl BlockNotice {
    fn bytes(c: &NoticeClaims) -> Vec<u8> {
        [BLOCK_DOMAIN, &ser(c)].concat()
    }
    fn verify(&self, me: &str) -> bool {
        let Some(key) = identity::public_key_from_did(&self.claims.owner) else {
            return false;
        };
        let Ok(sig) = <[u8; 64]>::try_from(self.signature.as_slice()) else {
            return false;
        };
        self.claims.blocked == me && identity::verify(&key, &Self::bytes(&self.claims), &sig)
    }
}

// ---- the sender: one install of the account ---------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Outgoing {
    List(SignedBlob),
    Block(BlockNotice),
}

/// One install of the account. Every install holds the mnemonic (`id`), its own current key, and
/// its view of the other live installs (synced between installs with lag, outside this model).
struct Install {
    id: Identity,
    key: SigningKey,
    others: Vec<Key>,
    own_list: Option<SignedBlob>,
    contacts: BTreeSet<String>,
    blocked: BTreeSet<String>,
    /// Latest item per contact; a newer list replaces an older queued one.
    outbox: BTreeMap<String, Outgoing>,
}

impl Install {
    fn new(id: Identity, key: SigningKey, contacts: &[String]) -> Self {
        Self {
            id,
            key,
            others: Vec::new(),
            own_list: None,
            contacts: contacts.iter().cloned().collect(),
            blocked: BTreeSet::new(),
            outbox: BTreeMap::new(),
        }
    }
    fn did(&self) -> String {
        self.id.did().to_string()
    }
    fn devices(&self) -> Vec<Key> {
        let mut v = vec![pk(&self.key)];
        v.extend(self.others.iter().copied().filter(|k| *k != pk(&self.key)));
        v
    }
    fn seq(&self) -> u64 {
        self.own_list
            .as_ref()
            .map_or(0, |b| list_seq(b, self.id.did()))
    }
    /// Re-sign (production rule `now.max(prev + 1)`, node.rs:2371) and queue for every
    /// unblocked contact (U8).
    fn resign(&mut self, now: u64) -> SignedBlob {
        let seq = now.max(self.seq() + 1);
        let entries: Vec<(Key, Option<String>)> =
            self.devices().into_iter().map(|k| (k, None)).collect();
        let blob = sign_device_list(&self.id, &entries, seq);
        self.own_list = Some(blob.clone());
        for c in &self.contacts {
            if !self.blocked.contains(c) {
                self.outbox.insert(c.clone(), Outgoing::List(blob.clone()));
            }
        }
        blob
    }
    fn add_device(&mut self, k: Key, now: u64) -> SignedBlob {
        self.others.push(k);
        self.resign(now)
    }
    fn unlink(&mut self, k: Key, now: u64) -> SignedBlob {
        self.others.retain(|o| *o != k);
        self.resign(now)
    }
    /// Restore-from-phrase on this install and a periodic private-endpoint rotation both replace
    /// this install's key; for the list they are the same change.
    fn replace_own_key(&mut self, k: SigningKey, now: u64) -> SignedBlob {
        self.key = k;
        self.resign(now)
    }
    /// U5 revised: one signed notice, no list, no re-sign, no rotation. Any queued list for that
    /// contact is replaced by the notice.
    fn block(&mut self, contact: &str, now: u64) {
        self.blocked.insert(contact.to_string());
        let claims = NoticeClaims {
            owner: self.did(),
            blocked: contact.to_string(),
            issued_at: now,
        };
        let signature = self.id.sign(&BlockNotice::bytes(&claims)).to_vec();
        self.outbox.insert(
            contact.to_string(),
            Outgoing::Block(BlockNotice { claims, signature }),
        );
    }
    /// The contact dialled us first ("catch-up"): we answer with our current list, or the notice.
    fn answer_dial(&mut self, contact: &str) -> Option<Outgoing> {
        self.outbox.remove(contact);
        if self.blocked.contains(contact) {
            self.block(contact, 0);
            return self.outbox.remove(contact);
        }
        if !self.contacts.contains(contact) {
            return None;
        }
        self.own_list.clone().map(Outgoing::List)
    }
    fn sign_content(&self, text: &str) -> Content {
        Content::sign(&self.key, text)
    }
}

// ---- content signed by a device key -----------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Content {
    author: Key,
    text: String,
    signature: Vec<u8>,
}

impl Content {
    fn sign(k: &SigningKey, text: &str) -> Self {
        let author = pk(k);
        let msg = [CONTENT_DOMAIN, &author, text.as_bytes()].concat();
        Self {
            author,
            text: text.into(),
            signature: k.sign(&msg).to_bytes().to_vec(),
        }
    }
    fn valid(&self) -> bool {
        let msg = [CONTENT_DOMAIN, &self.author, self.text.as_bytes()].concat();
        <[u8; 64]>::try_from(self.signature.as_slice())
            .is_ok_and(|s| identity::verify(&self.author, &msg, &s))
    }
}

// ---- the receiver: a contact's view of one owner ----------------------------------------------

/// What a receiver does when two verified lists carry the SAME seq with different contents.
/// Candidate G-FORK options; none is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum Fork {
    /// Shipped today (`proto::newer`): the larger signature wins the tie.
    TieBreakBySignature,
    /// Keep every list seen at the top seq; dial the union of their devices.
    UnionAtEqualSeq,
    /// Detect the fork, go back to the list held before it, refuse that seq until a higher one.
    FreezeAtEqualSeq,
}

#[derive(Debug, PartialEq, Eq)]
enum Reject {
    Invalid(proto::Error),
    NotNewer,
    Frozen,
    /// P4: seq more than `proto::MAX_LIST_SEQ_AHEAD_MS` ahead of the receiver's clock.
    TooFarAhead,
    UnknownAuthor,
    BadContentSignature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct View {
    me: String,
    owner: String,
    fork: Fork,
    /// Candidate G-HISTORY: keep keys dropped from earlier lists for verifying content.
    retain_superseded: bool,
    held: Option<SignedBlob>,
    devices: Vec<Key>,
    /// Union candidate: other lists held at the same top seq.
    siblings: Vec<SignedBlob>,
    /// Freeze candidate: the list before the top seq, and the frozen seq.
    prior: Option<(SignedBlob, Vec<Key>)>,
    frozen: Option<u64>,
    superseded: Vec<Key>,
    inbox: Vec<String>,
    notice: Option<BlockNotice>,
    /// The receiver's clock in the model's ticks (read as ms by `proto::list_seq_in_bounds`).
    now: u64,
}

impl View {
    fn new(me: &str, owner: &str) -> Self {
        Self {
            me: me.into(),
            owner: owner.into(),
            fork: Fork::TieBreakBySignature,
            retain_superseded: false,
            held: None,
            devices: Vec::new(),
            siblings: Vec::new(),
            prior: None,
            frozen: None,
            superseded: Vec::new(),
            inbox: Vec::new(),
            notice: None,
            now: 0,
        }
    }
    fn with_fork(mut self, f: Fork) -> Self {
        self.fork = f;
        self
    }
    fn snap(&self) -> Vec<u8> {
        ser(self)
    }
    fn held_seq(&self) -> Option<u64> {
        self.held.as_ref().map(|b| list_seq(b, &self.owner))
    }
    fn replace(&mut self, blob: &SignedBlob, keys: Vec<Key>) {
        for k in &self.devices {
            if !keys.contains(k) && !self.superseded.contains(k) {
                self.superseded.push(*k);
            }
        }
        if let Some(old) = self.held.take() {
            self.prior = Some((old, self.devices.clone()));
        }
        self.held = Some(blob.clone());
        self.devices = keys;
        self.siblings.clear();
    }
    /// Atomic: every `Err` leaves the view byte-for-byte unchanged.
    fn accept_list(&mut self, blob: &SignedBlob) -> Result<(), Reject> {
        let list = verify_device_list(blob, &self.owner).map_err(Reject::Invalid)?;
        // P4 (production `merge_device_list`): refuse a seq too far ahead of our clock.
        if !proto::list_seq_in_bounds(list.seq, self.now) {
            return Err(Reject::TooFarAhead);
        }
        let keys: Vec<Key> = list
            .devices
            .iter()
            .map(|e| e.device_key().unwrap())
            .collect();
        if self.frozen.is_some_and(|f| list.seq <= f) {
            return Err(Reject::Frozen);
        }
        let Some(held) = self.held.clone() else {
            self.replace(blob, keys);
            return Ok(());
        };
        let held_seq = self.held_seq().unwrap();
        match self.fork {
            Fork::TieBreakBySignature => {
                if !newer(blob, &held) {
                    return Err(Reject::NotNewer);
                }
                self.replace(blob, keys);
            }
            Fork::UnionAtEqualSeq => {
                if list.seq < held_seq || *blob == held || self.siblings.contains(blob) {
                    return Err(Reject::NotNewer);
                }
                if list.seq == held_seq {
                    for k in keys {
                        if !self.devices.contains(&k) {
                            self.devices.push(k);
                        }
                    }
                    self.devices.sort();
                    self.siblings.push(blob.clone());
                } else {
                    self.replace(blob, keys);
                }
            }
            Fork::FreezeAtEqualSeq => {
                if list.seq < held_seq || *blob == held {
                    return Err(Reject::NotNewer);
                }
                if list.seq == held_seq {
                    // Fork detected: back to the pre-fork list, refuse this seq.
                    let (b, d) = self.prior.clone().ok_or(Reject::Frozen)?;
                    self.held = Some(b);
                    self.devices = d;
                    self.frozen = Some(list.seq);
                } else {
                    self.replace(blob, keys);
                }
            }
        }
        Ok(())
    }
    /// All-or-nothing, like a history snapshot today.
    fn accept_batch(&mut self, batch: &[Content]) -> Result<(), Reject> {
        for c in batch {
            if !c.valid() {
                return Err(Reject::BadContentSignature);
            }
            let known = self.devices.contains(&c.author)
                || (self.retain_superseded && self.superseded.contains(&c.author));
            if !known {
                return Err(Reject::UnknownAuthor);
            }
        }
        self.inbox.extend(batch.iter().map(|c| c.text.clone()));
        Ok(())
    }
}

/// Hand the queued item for `view.me` to the contact (both online). `None`: nothing queued.
fn deliver(from: &mut Install, view: &mut View) -> Option<Result<(), Reject>> {
    let item = from.outbox.remove(&view.me)?;
    Some(receive(view, item))
}

fn receive(view: &mut View, item: Outgoing) -> Result<(), Reject> {
    match item {
        Outgoing::List(b) => view.accept_list(&b),
        Outgoing::Block(n) => {
            assert!(n.verify(&view.me), "notice verifies for its recipient");
            view.notice = Some(n);
            Ok(())
        }
    }
}

/// Bob's first install with contacts Alice, Carol, Dave; list signed at t=10 and delivered.
fn bob_with_contacts() -> (Install, View, View, View) {
    let contacts = [did(ALICE), did(CAROL), did(DAVE)];
    let mut b1 = Install::new(owner(BOB), dev(1, 0), &contacts);
    b1.resign(10);
    let mut views: Vec<View> = contacts.iter().map(|c| View::new(c, &did(BOB))).collect();
    for v in &mut views {
        deliver(&mut b1, v).unwrap().unwrap();
    }
    let dave = views.pop().unwrap();
    let carol = views.pop().unwrap();
    let alice = views.pop().unwrap();
    (b1, alice, carol, dave)
}

// ---- 1. every change is pushed to every unblocked contact -------------------------------------

// KIND: invariant
// U8: added, unlinked, restored, rotated -> each change re-signs with a higher seq and every
// unblocked contact ends up dialling exactly the new set. The blocked contact gets its notice
// once and never a list signed after the block; blocking itself re-signed nothing.
#[test]
fn every_list_change_reaches_every_unblocked_contact_and_never_the_blocked_one() {
    let (mut b1, mut alice, mut carol, mut dave) = bob_with_contacts();
    let seq_before_block = b1.seq();
    b1.block(&did(DAVE), 15);
    assert_eq!(
        b1.seq(),
        seq_before_block,
        "a block does not re-sign the list"
    );
    let dave_before = dave.snap();
    deliver(&mut b1, &mut dave).unwrap().unwrap();
    assert!(
        dave.notice.is_some(),
        "Dave is told he is blocked (U5 revised)"
    );
    dave.notice = None;
    assert_eq!(dave.snap(), dave_before, "the notice changed nothing else");

    let b2 = pk(&dev(2, 0));
    let mut last_seq = b1.seq();
    let steps: Vec<(&str, Box<dyn Fn(&mut Install) -> SignedBlob>)> = vec![
        ("add", Box::new(move |i: &mut Install| i.add_device(b2, 20))),
        ("unlink", Box::new(move |i: &mut Install| i.unlink(b2, 30))),
        (
            "restore",
            Box::new(|i: &mut Install| i.replace_own_key(dev(1, 1), 40)),
        ),
        (
            "rotate",
            Box::new(|i: &mut Install| i.replace_own_key(dev(1, 2), 50)),
        ),
    ];
    for (what, step) in steps {
        let blob = step(&mut b1);
        let seq = list_seq(&blob, &did(BOB));
        assert!(seq > last_seq, "{what}: seq goes up");
        last_seq = seq;
        assert!(
            !b1.outbox.contains_key(&did(DAVE)),
            "{what}: nothing queued for Dave"
        );
        for v in [&mut alice, &mut carol] {
            deliver(&mut b1, v).unwrap().unwrap();
            assert_eq!(v.held.as_ref(), Some(&blob), "{what}");
            assert_eq!(v.devices, b1.devices(), "{what}");
        }
        assert_eq!(
            deliver(&mut b1, &mut dave),
            None,
            "{what}: Dave gets nothing"
        );
    }
    assert_eq!(alice.devices, vec![pk(&dev(1, 2))]);
    assert_eq!(list_seq(dave.held.as_ref().unwrap(), &did(BOB)), 10);
}

// KIND: invariant
// A list queued for a contact that was offline is replaced by the notice when that contact is
// blocked: the contact never receives a list signed before the block either.
#[test]
fn blocking_replaces_a_queued_list_with_the_notice() {
    let (mut b1, _alice, _carol, mut dave) = bob_with_contacts();
    b1.add_device(pk(&dev(2, 0)), 20);
    assert!(matches!(b1.outbox.get(&did(DAVE)), Some(Outgoing::List(_))));
    b1.block(&did(DAVE), 21);
    let before = dave.snap();
    deliver(&mut b1, &mut dave).unwrap().unwrap();
    assert!(dave.notice.as_ref().unwrap().verify(&did(DAVE)));
    dave.notice = None;
    assert_eq!(
        dave.snap(),
        before,
        "Dave still holds the pre-block list only"
    );
    // Dave dialling first gets the same notice again, never the list.
    let again = b1.answer_dial(&did(DAVE)).unwrap();
    assert!(matches!(again, Outgoing::Block(_)));
}

// ---- 2. offline contacts ----------------------------------------------------------------------

// KIND: invariant
// Carol is offline across three changes. The outbox keeps only the latest list for her; when she
// is back and Bob's install retries, she jumps straight to the newest list.
#[test]
fn offline_contact_gets_only_the_latest_list_when_the_owner_retries() {
    let (mut b1, _alice, mut carol, _dave) = bob_with_contacts();
    b1.add_device(pk(&dev(2, 0)), 20);
    b1.add_device(pk(&dev(3, 0)), 30);
    let latest = b1.unlink(pk(&dev(2, 0)), 40);
    assert_eq!(
        b1.outbox.get(&did(CAROL)),
        Some(&Outgoing::List(latest.clone()))
    );
    deliver(&mut b1, &mut carol).unwrap().unwrap();
    assert_eq!(carol.held, Some(latest));
    assert_eq!(carol.devices, vec![pk(&dev(1, 0)), pk(&dev(3, 0))]);
    assert_eq!(deliver(&mut b1, &mut carol), None, "nothing left to send");
}

// KIND: invariant
// Carol dials first ("whoever reaches whom first"). She may reach a sibling install whose own
// list is older (own-device sync lag): she takes it if it is newer than hers and never regresses
// if it is older. The newest list arrives when she reaches the up-to-date install.
#[test]
fn offline_contact_dialling_first_converges_and_never_regresses() {
    let (mut b1, _alice, mut carol, _dave) = bob_with_contacts();
    let k2 = pk(&dev(2, 0));
    let mut b2 = Install::new(owner(BOB), dev(2, 0), &[did(CAROL)]);
    b1.add_device(k2, 20);
    // b2 syncs the registry once and signs its own list at t=21.
    b2.others = vec![pk(&dev(1, 0))];
    b2.resign(21);
    b2.outbox.clear();
    // b1 changes again; b2 does not hear of it.
    let newest = b1.add_device(pk(&dev(3, 0)), 30);

    // Carol reaches b2 first: its list (seq 21) is newer than hers (10).
    receive(&mut carol, b2.answer_dial(&did(CAROL)).unwrap()).unwrap();
    assert_eq!(set(&carol.devices), set(&[pk(&dev(1, 0)), k2]));
    // Then b1: newest.
    receive(&mut carol, b1.answer_dial(&did(CAROL)).unwrap()).unwrap();
    assert_eq!(carol.held.as_ref(), Some(&newest));
    // Reaching the stale b2 again: refused, unchanged.
    let before = carol.snap();
    let r = receive(&mut carol, b2.answer_dial(&did(CAROL)).unwrap());
    assert_eq!(r, Err(Reject::NotNewer));
    assert_eq!(carol.snap(), before);
}

// ---- 3. rejection ----------------------------------------------------------------------------

// KIND: invariant
// Stale (lower seq), replayed (same blob), forged (Mallory signs a list claiming Bob's DID),
// tampered, and wrong-DID (Mallory's own honest list offered as Bob's) lists are all refused,
// and the view is byte-for-byte unchanged.
#[test]
fn stale_replayed_forged_tampered_and_wrong_did_lists_leave_the_view_unchanged() {
    let (mut b1, mut alice, _carol, _dave) = bob_with_contacts();
    let old = b1.own_list.clone().unwrap();
    let cur = b1.add_device(pk(&dev(2, 0)), 20);
    deliver(&mut b1, &mut alice).unwrap().unwrap();

    // Forged: Mallory's key over a payload that claims Bob's DID.
    let mallory = owner(MALLORY);
    let mut forged = sign_device_list(&mallory, &[(pk(&dev(9, 0)), None)], 999);
    let claim: serde_json::Value = {
        let p = base64_decode(&forged.payload);
        let mut v: serde_json::Value = serde_json::from_slice(&p).unwrap();
        v["did"] = serde_json::Value::String(did(BOB));
        v
    };
    forged.payload = base64_encode(&serde_json::to_vec(&claim).unwrap());

    // Tampered: Bob's real signature, seq bumped.
    let mut tampered = cur.clone();
    let mut v: serde_json::Value = serde_json::from_slice(&base64_decode(&cur.payload)).unwrap();
    v["seq"] = serde_json::json!(5000);
    tampered.payload = base64_encode(&serde_json::to_vec(&v).unwrap());

    let wrong_did = sign_device_list(&mallory, &[(pk(&dev(9, 0)), None)], 999);

    let cases: [(&str, SignedBlob, Reject); 5] = [
        ("stale", old, Reject::NotNewer),
        ("replayed", cur.clone(), Reject::NotNewer),
        (
            "forged",
            forged,
            Reject::Invalid(proto::Error::BadSignature),
        ),
        (
            "tampered",
            tampered,
            Reject::Invalid(proto::Error::BadSignature),
        ),
        (
            "wrong did",
            wrong_did,
            Reject::Invalid(proto::Error::WrongIssuer),
        ),
    ];
    for (what, blob, want) in cases {
        let before = alice.snap();
        assert_eq!(alice.accept_list(&blob), Err(want), "{what}");
        assert_eq!(alice.snap(), before, "{what}: unchanged");
    }
    assert_eq!(alice.held, Some(cur));
}

// Minimal URL-safe no-pad base64 so the model can tamper with payloads without new deps.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |a, b| (a << 8) | *b as u32) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}
fn base64_decode(text: &str) -> Vec<u8> {
    let vals: Vec<u32> = text
        .bytes()
        .map(|c| B64.iter().position(|x| *x == c).unwrap() as u32)
        .collect();
    let mut out = Vec::new();
    for chunk in vals.chunks(4) {
        let n = chunk.iter().fold(0u32, |a, b| (a << 6) | b) << (6 * (4 - chunk.len()));
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

// ---- 4. restore (K1 stall) --------------------------------------------------------------------

// KIND: invariant
// K1 §5 bug 1 without a pending message: Alice holds Bob's list naming only his old key. After
// Bob restores (new key), content signed by the new key is refused until the pushed list
// arrives, and accepted right after. Pushing the list before (or with) new content clears it.
#[test]
fn restore_push_lets_the_contact_accept_content_from_the_new_key() {
    let (mut b1, mut alice, _carol, _dave) = bob_with_contacts();
    b1.replace_own_key(dev(1, 1), 20);
    let m3 = b1.sign_content("m3 after restore");
    let before = alice.snap();
    assert_eq!(
        alice.accept_batch(&[m3.clone()]),
        Err(Reject::UnknownAuthor)
    );
    assert_eq!(alice.snap(), before, "the K1 stall without a push");
    deliver(&mut b1, &mut alice).unwrap().unwrap();
    assert_eq!(alice.accept_batch(&[m3]), Ok(()));
    assert_eq!(alice.inbox, vec!["m3 after restore".to_string()]);
}

// KIND: limitation
// K1 §5 bug 1 WITH a pending message: `m2` was signed by the old key before the restore and is
// still undelivered; the next history batch carries `m2` and `m3`. With replace-only semantics the
// pushed list drops the old key, so the batch fails as a whole even after the push: U8's push
// alone does not clear this stall. Expected-correct: both reach Alice (see the G-HISTORY candidate
// below, or the sender re-signing/splitting, or the batch accepting per message).
#[test]
fn restore_push_alone_does_not_clear_a_batch_holding_a_pre_restore_message() {
    let (mut b1, mut alice, _carol, _dave) = bob_with_contacts();
    let m2 = b1.sign_content("m2 pending at restore");
    b1.replace_own_key(dev(1, 1), 20);
    let m3 = b1.sign_content("m3 after restore");
    deliver(&mut b1, &mut alice).unwrap().unwrap();
    let before = alice.snap();
    assert_eq!(alice.accept_batch(&[m2, m3]), Err(Reject::UnknownAuthor));
    assert_eq!(alice.snap(), before);
}

// KIND: candidate-policy G-HISTORY
// Option: a receiver keeps keys that earlier lists of the same DID named, for verifying content
// only (never for dialling). The pending batch then goes through.
#[test]
fn retaining_superseded_keys_accepts_the_pre_restore_batch() {
    let (mut b1, mut alice, _carol, _dave) = bob_with_contacts();
    alice.retain_superseded = true;
    let m2 = b1.sign_content("m2 pending at restore");
    b1.replace_own_key(dev(1, 1), 20);
    let m3 = b1.sign_content("m3 after restore");
    deliver(&mut b1, &mut alice).unwrap().unwrap();
    assert_eq!(
        alice.devices,
        vec![pk(&dev(1, 1))],
        "the old key is not dialled"
    );
    assert_eq!(alice.accept_batch(&[m2, m3]), Ok(()));
}

// KIND: limitation
// The cost of the candidate above: without trusted time, "content written before the list
// changed" cannot be told from new content by the old key. An unlinked install (it still holds
// its key, and the mnemonic) keeps authoring accepted content. Shared-mnemonic: not revocation.
#[test]
fn retained_superseded_keys_also_accept_new_content_from_an_unlinked_install() {
    let (mut b1, mut alice, _carol, _dave) = bob_with_contacts();
    alice.retain_superseded = true;
    let k2 = dev(2, 0);
    b1.add_device(pk(&k2), 20);
    deliver(&mut b1, &mut alice).unwrap().unwrap();
    b1.unlink(pk(&k2), 30);
    deliver(&mut b1, &mut alice).unwrap().unwrap();
    assert!(!alice.devices.contains(&pk(&k2)));
    let late = Content::sign(&k2, "written after the unlink");
    assert_eq!(alice.accept_batch(&[late]), Ok(()));
}

// ---- 5. concurrent edits by two own installs (G-FORK) -----------------------------------------

/// Bob has b1, b2, b3 in a list at t=100 held by `views`. At the same tick 200 b1 unlinks b3 and
/// b2 adds b4 (neither has heard of the other's edit). Returns (b1's list, b2's list, b1, b2).
fn concurrent_edits() -> (SignedBlob, SignedBlob, Install, Install) {
    let (k1, k2, k3, k4) = (
        pk(&dev(1, 0)),
        pk(&dev(2, 0)),
        pk(&dev(3, 0)),
        pk(&dev(4, 0)),
    );
    let contacts = [did(ALICE), did(CAROL)];
    let mut b1 = Install::new(owner(BOB), dev(1, 0), &contacts);
    b1.others = vec![k2, k3];
    let mut b2 = Install::new(owner(BOB), dev(2, 0), &contacts);
    b2.others = vec![k1, k3];
    b1.resign(100);
    b2.resign(100);
    let l1 = b1.unlink(k3, 200);
    let l2 = b2.add_device(k4, 200);
    assert_eq!(
        list_seq(&l1, &did(BOB)),
        list_seq(&l2, &did(BOB)),
        "same seq"
    );
    assert_ne!(
        set(&list_keys(&l1, &did(BOB))),
        set(&list_keys(&l2, &did(BOB)))
    );
    (l1, l2, b1, b2)
}

fn base_view(me: u8, fork: Fork) -> View {
    let (k1, k2, k3) = (pk(&dev(1, 0)), pk(&dev(2, 0)), pk(&dev(3, 0)));
    let mut v = View::new(&did(me), &did(BOB)).with_fork(fork);
    let base = sign_device_list(&owner(BOB), &[(k1, None), (k2, None), (k3, None)], 100);
    v.accept_list(&base).unwrap();
    v
}

/// Alice receives l1 then l2, Carol l2 then l1.
fn both_orders(fork: Fork, l1: &SignedBlob, l2: &SignedBlob) -> (View, View) {
    let mut alice = base_view(ALICE, fork);
    let mut carol = base_view(CAROL, fork);
    let _ = alice.accept_list(l1);
    let _ = alice.accept_list(l2);
    let _ = carol.accept_list(l2);
    let _ = carol.accept_list(l1);
    (alice, carol)
}

// KIND: candidate-policy G-FORK
// Shipped tie-break (`proto::newer`: larger signature wins at equal seq). Every receiver lands on
// the same list whatever the order, but exactly one of the two edits is lost: either b3 is
// dialled again (the unlink lost) or b4 is not dialled (the add lost). Nobody is told.
#[test]
fn fork_tie_break_by_signature_converges_but_drops_one_edit() {
    let (l1, l2, _, _) = concurrent_edits();
    let (alice, carol) = both_orders(Fork::TieBreakBySignature, &l1, &l2);
    assert_eq!(alice.held, carol.held, "order-independent");
    let winner = if newer(&l1, &l2) { &l1 } else { &l2 };
    assert_eq!(alice.held.as_ref(), Some(winner));
    let d = set(&alice.devices);
    let unlink_lost = d.contains(&pk(&dev(3, 0)));
    let add_lost = !d.contains(&pk(&dev(4, 0)));
    assert!(
        unlink_lost ^ add_lost,
        "exactly one edit lost: {unlink_lost} {add_lost}"
    );
}

// KIND: candidate-policy G-FORK
// Union at equal seq: order-independent and the add survives, but a removal cannot be expressed
// by a union: the unlinked b3 is dialled again.
#[test]
fn fork_union_merge_keeps_the_add_and_resurrects_the_unlinked_device() {
    let (l1, l2, _, _) = concurrent_edits();
    let (alice, carol) = both_orders(Fork::UnionAtEqualSeq, &l1, &l2);
    assert_eq!(set(&alice.devices), set(&carol.devices));
    let want = set(&[
        pk(&dev(1, 0)),
        pk(&dev(2, 0)),
        pk(&dev(3, 0)),
        pk(&dev(4, 0)),
    ]);
    assert_eq!(set(&alice.devices), want, "b3 resurrected, b4 kept");
}

// KIND: candidate-policy G-FORK
// Freeze: the fork is detected (same seq, different content); every receiver goes back to the
// pre-fork list and refuses that seq until a strictly higher list arrives. Order-independent and
// loses no edit silently, but any phrase holder can trigger it at will (DoS, see Q4).
#[test]
fn fork_freeze_reverts_to_the_pre_fork_list_until_a_higher_seq() {
    let (l1, l2, mut b1, _) = concurrent_edits();
    let (mut alice, carol) = both_orders(Fork::FreezeAtEqualSeq, &l1, &l2);
    for v in [&alice, &carol] {
        assert_eq!(v.frozen, Some(200));
        assert_eq!(
            set(&v.devices),
            set(&[pk(&dev(1, 0)), pk(&dev(2, 0)), pk(&dev(3, 0))])
        );
    }
    assert_eq!(alice.devices, carol.devices);
    let before = alice.snap();
    assert_eq!(alice.accept_list(&l1), Err(Reject::Frozen));
    assert_eq!(alice.snap(), before);
    let resolved = b1.resign(300);
    alice.accept_list(&resolved).unwrap();
    assert_eq!(alice.held, Some(resolved));
}

// KIND: candidate-policy G-FORK
// Sender-side resolution: once the two installs exchange registries (own-device sync with
// tombstones, removal wins: itself a candidate), each re-signs with a higher seq and every
// contact converges on a list with both edits. Between t=200 and the re-sign, contacts hold a
// list missing one edit (the tie-break above).
#[test]
fn fork_resolved_by_re_signing_after_own_device_sync_keeps_both_edits() {
    let (l1, l2, mut b1, mut b2) = concurrent_edits();
    let (mut alice, mut carol) = both_orders(Fork::TieBreakBySignature, &l1, &l2);
    let (k1, k2, k3, k4) = (
        pk(&dev(1, 0)),
        pk(&dev(2, 0)),
        pk(&dev(3, 0)),
        pk(&dev(4, 0)),
    );
    let tombstones = [k3];
    let merged: Vec<Key> = [k1, k2, k3, k4]
        .into_iter()
        .filter(|k| !tombstones.contains(k))
        .collect();
    b1.others = merged.iter().copied().filter(|k| *k != k1).collect();
    b2.others = merged.iter().copied().filter(|k| *k != k2).collect();
    let r1 = b1.resign(300);
    let r2 = b2.resign(301);
    for v in [&mut alice, &mut carol] {
        let _ = v.accept_list(&r2);
        let _ = v.accept_list(&r1);
        assert_eq!(v.held.as_ref(), Some(&r2), "latest re-sign wins");
        assert_eq!(set(&v.devices), set(&merged));
    }
}

// KIND: limitation
// With production's ms-clock seq the two edits rarely tie: b2 adds b4 one tick after b1 unlinked
// b3, without having heard of it. Every contact takes b2's list (higher seq), so the unlink is
// silently undone at contacts (b3 dialled again) until some install re-signs after own sync. No
// receiver can detect this as a fork. Expected-correct: POLICY-GATED (G-FORK).
#[test]
fn edits_with_distinct_seq_silently_undo_an_unlink_at_contacts() {
    let (k1, k2, k3, k4) = (
        pk(&dev(1, 0)),
        pk(&dev(2, 0)),
        pk(&dev(3, 0)),
        pk(&dev(4, 0)),
    );
    let mut b1 = Install::new(owner(BOB), dev(1, 0), &[did(ALICE)]);
    b1.others = vec![k2, k3];
    let mut b2 = Install::new(owner(BOB), dev(2, 0), &[did(ALICE)]);
    b2.others = vec![k1, k3];
    let mut alice = View::new(&did(ALICE), &did(BOB));
    receive(&mut alice, Outgoing::List(b1.resign(100))).unwrap();
    b2.resign(100);
    receive(&mut alice, Outgoing::List(b1.unlink(k3, 200))).unwrap();
    assert!(!alice.devices.contains(&k3));
    receive(&mut alice, Outgoing::List(b2.add_device(k4, 201))).unwrap();
    assert!(alice.devices.contains(&k3), "the unlink is undone");
    assert!(
        alice.frozen.is_none() && alice.siblings.is_empty(),
        "nothing detected"
    );
}

// ---- 6. shared mnemonic -----------------------------------------------------------------------

// KIND: limitation
// Any phrase holder (a hostile linked install, a thief) signs a valid list that drops every
// honest install; contacts accept it and stop dialling Bob's real devices. An honest re-sign at a
// later tick wins it back, so this is a seq war, not a revocation of the holder.
#[test]
fn any_phrase_holder_can_push_a_list_that_drops_the_honest_installs() {
    let (mut b1, mut alice, _carol, _dave) = bob_with_contacts();
    let thief = Install::new(owner(BOB), dev(7, 0), &[did(ALICE)]);
    let hostile = sign_device_list(&thief.id, &[(pk(&thief.key), None)], 500);
    alice.accept_list(&hostile).unwrap();
    assert_eq!(
        alice.devices,
        vec![pk(&dev(7, 0))],
        "Bob's real install is no longer dialled"
    );
    let honest = b1.resign(600);
    deliver(&mut b1, &mut alice).unwrap().unwrap();
    assert_eq!(alice.held, Some(honest));
}

// KIND: invariant (was a limitation; fixed by P4)
// A holder signs at `seq = u64::MAX`. Before P4 contacts took it and no later list could be newer
// (`newer` compares seq first): pinned forever. Now the receiver refuses any seq more than
// `proto::MAX_LIST_SEQ_AHEAD_MS` (one day, PROVISIONAL (coordinator)) ahead of its clock, so the
// view is unchanged and the honest list still wins. Residual: a holder can still sign up to a day
// ahead, which wins until honest lists catch up with the clock (a one-day seq war, not forever).
#[test]
fn a_max_seq_list_is_refused_by_the_seq_bound() {
    let (mut b1, mut alice, _carol, _dave) = bob_with_contacts();
    let thief = owner(BOB);
    let pinned = sign_device_list(&thief, &[(pk(&dev(7, 0)), None)], u64::MAX);
    let before = alice.snap();
    assert_eq!(alice.accept_list(&pinned), Err(Reject::TooFarAhead));
    assert_eq!(alice.snap(), before);
    let ahead = sign_device_list(&thief, &[(pk(&dev(7, 0)), None)], proto::MAX_LIST_SEQ_AHEAD_MS + 1);
    assert_eq!(alice.accept_list(&ahead), Err(Reject::TooFarAhead));
    let honest = b1.resign(1_000_000);
    deliver(&mut b1, &mut alice).unwrap().unwrap();
    assert_eq!(alice.held, Some(honest));
}

// ---- 7. block exclusion depends on the block reaching every own install -----------------------

// KIND: limitation
// Exclusion is enforced by the sending install. b1 blocks Dave; b2 has not yet received the block
// (own-device sync lag) and adds a device: its push reaches Dave, who learns the new set after
// being blocked. Expected-correct: POLICY-GATED (G-BLOCK: hold pushes until the block list is
// synced, or accept the window as with W2-B's sibling-rotation leak).
#[test]
fn a_sibling_that_has_not_heard_of_the_block_pushes_to_the_blocked_contact() {
    let (mut b1, _alice, _carol, mut dave) = bob_with_contacts();
    let mut b2 = Install::new(owner(BOB), dev(2, 0), &[did(ALICE), did(CAROL), did(DAVE)]);
    b2.others = vec![pk(&dev(1, 0))];
    b1.block(&did(DAVE), 20);
    deliver(&mut b1, &mut dave).unwrap().unwrap();
    assert!(dave.notice.is_some());
    let leaked = b2.add_device(pk(&dev(3, 0)), 21);
    deliver(&mut b2, &mut dave).unwrap().unwrap();
    assert_eq!(
        dave.held,
        Some(leaked),
        "Dave got a list signed after he was blocked"
    );
    assert!(dave.devices.contains(&pk(&dev(3, 0))));
}
