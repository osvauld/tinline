//! The batch rules of `doc.rs` (P9c, U18: strict, no transition). Every message version carries
//! its author's DID signature and the writing install's device ID as record fields, bound to the
//! span of ops that wrote it; every op of every batch must lie in such a proven span under the
//! writer ID `writer_id(author, device ID)`. Device keys play no part.
//!
//! Every test carries one label: `KIND: invariant` (holds and must keep holding) or
//! `KIND: limitation` (an accepted or policy-gated gap, stated in its comment).

use super::doc::*;
use loro::{ExportMode, LoroDoc, LoroMap};

const DAY: &str = "2026-10-07";
const NOON: i64 = 1_791_374_400_000; // 2026-10-07 12:00:00 UTC

/// Accounts with real `did:key` DIDs (0 and 1 talk; 2 is a stranger).
fn acct(n: usize) -> &'static identity::Identity {
    static IDS: std::sync::OnceLock<Vec<identity::Identity>> = std::sync::OnceLock::new();
    &IDS.get_or_init(|| (0..3).map(|_| identity::generate().0).collect())[n]
}

fn did(n: usize) -> String {
    acct(n).did().to_string()
}

/// A device ID (random per install in the app; fixed here).
fn val(v: u8) -> [u8; 32] {
    [100 + v; 32]
}

fn wid(n: usize, v: u8) -> u64 {
    writer_id(&did(n), &val(v))
}

/// Account `n` on the install with device ID `v`.
fn w(n: usize, v: u8) -> Writer {
    Writer::new(acct(n).clone(), val(v))
}

/// The shard of account `n` on install `v` (it writes under its writer ID).
fn shard(n: usize, v: u8) -> Shard {
    Shard::new("p", DAY, wid(n, v))
}

/// A receiver that only reads in these tests.
fn recv() -> Shard {
    Shard::new("p", DAY, 1)
}

fn name() -> String {
    doc_name("p", DAY)
}

fn msg(id: &str, author: &str, at: i64, text: &str) -> MsgRec {
    MsgRec { id: id.into(), author: author.into(), at, text: text.into(), edited_at: None, deleted: false, reply_to: None, file: None, dev: [0; 32], did_sig: Vec::new() }
}

/// A live push from account `signer`.
fn live(signer: usize) -> Ctx {
    Ctx { signer_did: did(signer), now_ms: NOON, live: true, history: None }
}

/// A catch-up `Sync` from account `signer` (no 24 h window).
fn sync(signer: usize) -> Ctx {
    Ctx { live: false, ..live(signer) }
}

/// A vouched history snapshot from the contact `contact`, received by `me`.
fn hist(contact: usize, me: usize) -> Ctx {
    Ctx { signer_did: did(contact), now_ms: NOON, live: false, history: Some(did(me)) }
}

/// Own-device sync of account `me` about the conversation with `contact` (`chat_own_apply`).
fn own(me: usize, contact: usize) -> Ctx {
    Ctx { signer_did: did(me), now_ms: NOON, live: false, history: Some(did(contact)) }
}

/// A raw doc seeded with `from`'s content, writing as `peer`.
fn seeded(from: &Shard, peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(&from.snapshot()).unwrap();
    doc.set_peer_id(peer).unwrap();
    doc
}

/// Raw ops under `peer` on a doc seeded with `from`; returns the update they make.
fn forge(from: &Shard, peer: u64, f: impl FnOnce(&LoroMap)) -> Vec<u8> {
    let doc = seeded(from, peer);
    let before = doc.oplog_vv();
    f(&doc.get_map("messages"));
    doc.commit();
    doc.export(ExportMode::updates(&before)).unwrap()
}

fn entry(map: &LoroMap, id: &str) -> Option<LoroMap> {
    match map.get(id) {
        Some(loro::ValueOrContainer::Container(loro::Container::Map(m))) => Some(m),
        _ => None,
    }
}

fn set(map: &LoroMap, id: &str, k: &str, v: impl Into<loro::LoroValue>) {
    entry(map, id).unwrap().insert(k, v).unwrap();
}

/// Writes `rec` under `peer` (a doc seeded with `from`) as one span the way a writer does: the
/// immutable fields if `from` does not hold it, then the version under `peer`'s version key with
/// `did_sig = sign(rec, where the span really is)`.
fn span(from: &Shard, peer: u64, rec: &MsgRec, sign: impl FnOnce(&MsgRec, &WrittenAt) -> Vec<u8>) -> Vec<u8> {
    let doc = seeded(from, peer);
    let before = doc.oplog_vv();
    let map = doc.get_map("messages");
    let e = match entry(&map, &rec.id) {
        Some(e) => e,
        None => {
            let e = map.insert_container(&rec.id, LoroMap::new()).unwrap();
            e.insert("id", rec.id.as_str()).unwrap();
            e.insert("author", rec.author.as_str()).unwrap();
            e.insert("at", rec.at).unwrap();
            e
        }
    };
    let at = WrittenAt { peer, counter: before.get(&peer).copied().unwrap_or(0), len: doc.get_pending_txn_len() as u32 + 1 };
    e.insert(&version_key(peer), version_value(rec, &sign(rec, &at))).unwrap();
    assert_eq!(doc.get_pending_txn_len() as u32, at.len);
    doc.commit();
    doc.export(ExportMode::updates(&before)).unwrap()
}

/// Signed by account `n`'s DID key, as a writer signs.
fn by(n: usize) -> impl FnOnce(&MsgRec, &WrittenAt) -> Vec<u8> {
    move |r, at| sign_msg(acct(n), &name(), r, at)
}

/// `rec` written by account `n` on install `v`.
fn rec_of(id: &str, n: usize, v: u8, at: i64, text: &str) -> MsgRec {
    MsgRec { dev: val(v), ..msg(id, &did(n), at, text) }
}

/// The peer ids an update holds ops of.
fn peers_of(u: &[u8]) -> Vec<u64> {
    LoroDoc::decode_import_blob_meta(u, true).unwrap().partial_end_vv.keys().copied().collect()
}

/// A doc holding `updates`, plus raw ops `f` under `peer`; returns all of it (self-contained).
fn doc_with(updates: &[&[u8]], peer: u64, f: impl FnOnce(&LoroMap)) -> Vec<u8> {
    let doc = LoroDoc::new();
    for u in updates {
        doc.import(u).unwrap();
    }
    doc.set_peer_id(peer).unwrap();
    f(&doc.get_map("messages"));
    doc.commit();
    doc.export(ExportMode::all_updates()).unwrap()
}

/// Ops on the record `id` that leave it as it reads (a field set and removed again).
fn touch(map: &LoroMap, id: &str) {
    let e = entry(map, id).unwrap();
    e.insert("x", 1).unwrap();
    e.delete("x").unwrap();
}

mod basics;
mod forgeries;
mod honest;
mod relay;
mod writer_ids;
