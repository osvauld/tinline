//! Chats between a person's own devices (task 34g). The pieces the `tinline/self/1` session
//! (`sync/engine.rs`) calls: which shards we offer, exporting and validating a batch relayed by
//! another own device, read cursors, and who may fetch blobs. See `docs/chat.md` "Own devices".

use std::collections::HashMap;
use std::sync::Arc;

use loro::VersionVector;

use super::doc::*;
use super::engine::{ChatCore, decode_vv};
use super::store::ConvMeta;
use crate::Error;
use crate::node::{Inner, Me, now_ms};

/// The dirty-set entry that means "a read cursor moved".
pub(crate) const READ_MARK: &str = "!read";
/// The dirty-set entry that means "a contact acknowledged more".
pub(crate) const ACK_MARK: &str = "!ack";
/// Shards offered per session (newest days first); the rest are not synced until the next
/// session finds room (older days are normally closed and rarely change).
pub(crate) const MAX_OWN_DOCS: usize = 4096;
/// A batch bigger than this is not sent over the own-device link (the frame cap is 16 MiB).
const MAX_OWN_BATCH: usize = 12 * 1024 * 1024;

/// `dm/{pair}/{day}` -> `(pair, day)`.
pub(crate) fn parse_doc(doc: &str) -> Option<(String, String)> {
    let rest = doc.strip_prefix("dm/")?;
    let (pair, day) = rest.split_once('/')?;
    (pair.len() == 64 && pair.bytes().all(|b| b.is_ascii_hexdigit()) && day_start(day).is_some()).then(|| (pair.to_string(), day.to_string()))
}

impl Inner {
    /// Another install of this account that is not removed (it may read our blobs).
    pub(crate) fn is_own_device(&self, dev: &[u8; 32]) -> bool {
        let s = self.shared.lock();
        s.me.as_ref().is_some_and(|m| m.device != *dev) && s.state.registry.iter().any(|e| e.device == *dev && !e.removed)
    }

    /// Our other devices as `(device, relay, in sync with it right now)`.
    pub(crate) fn own_blob_sources(&self) -> Vec<([u8; 32], Option<String>, bool)> {
        let list: Vec<([u8; 32], Option<String>)> = {
            let s = self.shared.lock();
            let Some(me) = s.me.as_ref() else { return vec![] };
            s.state.registry.iter().filter(|e| !e.removed && e.device != me.device).map(|e| (e.device, e.relay.clone())).collect()
        };
        let sessions = self.selfsync.sessions.lock();
        list.into_iter().map(|(d, r)| (d, r, sessions.contains_key(&d))).collect()
    }

    /// Attested device keys by DID for judging messages that did not come straight from their
    /// author: all our own devices (removed ones too, their old messages stay valid) and every
    /// device we know of the contact.
    pub(crate) fn chat_keys(&self, me: &Me, contact: &str) -> Vec<(String, [u8; 32])> {
        let s = self.shared.lock();
        let mut keys = vec![(me.id.did().to_string(), me.device)];
        keys.extend(s.state.registry.iter().map(|e| (me.id.did().to_string(), e.device)));
        if let Some(c) = s.state.contacts.iter().find(|c| c.did == contact) {
            keys.extend(c.devices.iter().map(|d| (c.did.clone(), d.device)));
        }
        keys
    }

    /// `pair` -> the DID of that conversation's other side (ourselves for "You"), if it is a
    /// conversation of ours right now.
    pub(crate) fn pair_owner(&self, me: &Me, pair: &str) -> Option<String> {
        if pair_id(me.id.did(), me.id.did()) == pair {
            return Some(me.id.did().to_string());
        }
        let s = self.shared.lock();
        s.state.contacts.iter().find(|c| pair_id(me.id.did(), &c.did) == pair && !s.state.blocked.contains(&c.did)).map(|c| c.did.clone())
    }

    /// Marks `name` (a doc, or `READ_MARK`) as changed for every own-device session and wakes them.
    pub(crate) fn selfsync_dirty(&self, name: &str) {
        {
            let sessions = self.selfsync.sessions.lock();
            if sessions.is_empty() {
                return;
            }
            for s in sessions.values() {
                s.dirty.lock().insert(name.to_string());
            }
        }
        self.sync_bump();
    }

    /// The shards we offer another own device: `(doc, vv)`, newest day first.
    pub(crate) fn chat_own_docs(&self) -> Vec<(String, Vec<u8>)> {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return vec![] };
        let Ok(all) = core.store.all_shards(MAX_OWN_DOCS) else { return vec![] };
        let mut owners: HashMap<String, bool> = HashMap::new();
        all.into_iter()
            .filter(|(pair, _, _)| *owners.entry(pair.clone()).or_insert_with(|| self.pair_owner(&me, pair).is_some()))
            .map(|(pair, day, vv)| (doc_name(&pair, &day), vv))
            .collect()
    }

    /// What we hold of `doc` beyond `theirs`, with our version vector; `None` if they have it
    /// all (or the doc is not one of our conversations).
    pub(crate) async fn chat_own_export(self: &Arc<Self>, doc: &str, theirs: &VersionVector) -> Result<Option<(Vec<u8>, Vec<u8>)>, Error> {
        let (me, core) = (self.me()?, self.chat_core()?);
        let Some((pair, day)) = parse_doc(doc) else { return Ok(None) };
        if self.pair_owner(&me, &pair).is_none() {
            return Ok(None);
        }
        let Some(meta) = core.store.shard_meta(&pair, &day)? else { return Ok(None) };
        let mine = VersionVector::decode(&meta.vv).map_err(|_| Error::Io("bad stored version vector".into()))?;
        if theirs.partial_cmp(&mine).is_some_and(|o| o != std::cmp::Ordering::Less) {
            return Ok(None);
        }
        self.chat_shard(&core, &me, &pair, &day).await?;
        let out = core.shards.lock().get(&(pair, day)).and_then(|s| s.export_all_since(theirs).map(|u| (u, s.vv().encode())));
        match out {
            Some((u, _)) if u.len() > MAX_OWN_BATCH => {
                tracing::warn!("own-device sync: {doc} is too big to send ({} bytes)", u.len());
                Ok(None)
            }
            other => Ok(other),
        }
    }

    /// Judges and stores a batch of `doc` relayed by another own device (its signature was
    /// checked by the caller). The batch may hold ops of any of our devices and of the contact's
    /// devices; each new or changed message must be signed by its author's attested device (the
    /// rules of a vouched history snapshot), authors are only us and the contact, and the usual
    /// per-field rules hold. `Err` = rejected, nothing stored.
    pub(crate) async fn chat_own_apply(self: &Arc<Self>, doc: &str, update: &[u8]) -> Result<(), Error> {
        let (me, core) = (self.me()?, self.chat_core()?);
        let (pair, day) = parse_doc(doc).ok_or_else(|| Error::Protocol("bad doc".into()))?;
        // Not (or no longer) a conversation of ours, e.g. a contact removed meanwhile: nothing to do.
        let Some(did) = self.pair_owner(&me, &pair) else { return Ok(()) };
        self.chat_shard(&core, &me, &pair, &day).await?;
        let keys = self.chat_keys(&me, &did);
        let committed = {
            let mut shards = core.shards.lock();
            let shard = shards.get_mut(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
            let before = shard.vv();
            let ctx = Ctx {
                signer_did: me.id.did().to_string(),
                signer_peer: 0,
                now_ms: now_ms() as i64,
                live: false,
                history: Some(did.clone()),
                keys,
            };
            match shard.apply_remote(update, &ctx) {
                Ok(applied) => {
                    if shard.vv() == before {
                        None
                    } else {
                        let committed = self.chat_commit(&core, &me, &did, &pair, &day, shard, update, &applied, true)?;
                        // Our messages written on the other device: remember which change the
                        // contact must hold before they get two ticks here too.
                        if !self.is_self_did(&did) {
                            let mut ops = Vec::new();
                            for (rec, added) in &committed.0 {
                                if *added
                                    && rec.author == me.id.did()
                                    && let Some((peer, c)) = shard.creator_end(&rec.id)
                                    && peer != shard.peer
                                {
                                    ops.push(core.store.put_mo_op(&pair, &day, &rec.id, peer, c)?);
                                }
                            }
                            if !ops.is_empty() {
                                core.store.db.apply(&ops).map_err(crate::chat::store::io)?;
                            }
                        }
                        Some(committed)
                    }
                }
                // Ops that build on ops we lack: the sender's idea of our version was stale; the
                // next hello fixes it.
                Err(Reject::Pending) => None,
                Err(r) => {
                    self.log(format!("rejected own-device batch for {doc}: {r}"));
                    return Err(Error::Protocol(format!("rejected own-device batch: {r}")));
                }
            }
        };
        let Some((recs, conv, files)) = committed else { return Ok(()) };
        if let Some(ev) = self.ev() {
            for (rec, added) in &recs {
                let m = self.chat_api_message(&core, &me, &did, &pair, &day, rec);
                if *added { ev.on_message_added(m) } else { ev.on_message_changed(m) }
            }
            if let Some(c) = self.chat_api_chat(&core, &me, &conv) {
                ev.on_chat_changed(c);
            }
        }
        for h in files {
            self.chat_download(did.clone(), h);
        }
        Ok(())
    }

    // ---- read cursors --------------------------------------------------------------------------

    /// `(pair, cursor)` of every conversation with a cursor.
    pub(crate) fn chat_read_cursors(&self) -> Vec<(String, i64)> {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return vec![] };
        core.store
            .convs()
            .unwrap_or_default()
            .into_iter()
            .filter(|(pair, c)| c.read_upto > 0 && self.pair_owner(&me, pair).is_some())
            .map(|(pair, c)| (pair, c.read_upto))
            .collect()
    }

    /// Cursors from another own device: a larger one wins (a max-register), and the unread count
    /// is recomputed against it, so the order in which messages and cursors arrive does not matter.
    pub(crate) async fn chat_apply_read(self: &Arc<Self>, cursors: &[(String, i64)]) -> Result<(), Error> {
        let (me, core) = (self.me()?, self.chat_core()?);
        let mut moved = false;
        for (pair, cursor) in cursors {
            let Some(did) = self.pair_owner(&me, pair) else { continue };
            let mut conv = core.store.conv(pair)?.unwrap_or_else(|| ConvMeta { peer_did: did.clone(), ..Default::default() });
            if *cursor <= conv.read_upto {
                continue;
            }
            conv.peer_did = did.clone();
            conv.read_upto = *cursor;
            conv.unread = self.chat_count_unread(&core, &me, pair, *cursor).await?;
            core.store.db.apply(&[core.store.conv_op(pair, &conv)?]).map_err(super::store::io)?;
            moved = true;
            if let (Some(ev), Some(c)) = (self.ev(), self.chat_api_chat(&core, &me, &conv)) {
                ev.on_chat_changed(c);
            }
        }
        if moved {
            // Pass it on to our other devices (the sessions only send what a device lacks).
            self.selfsync_dirty(READ_MARK);
        }
        Ok(())
    }

    /// `(doc, vv)` of every day a contact of ours acknowledged.
    pub(crate) fn chat_acks(&self) -> Vec<(String, VersionVector)> {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return vec![] };
        core.store
            .acks()
            .unwrap_or_default()
            .into_iter()
            .filter(|(pair, _, _)| self.pair_owner(&me, pair).is_some())
            .filter_map(|(pair, day, vv)| Some((doc_name(&pair, &day), decode_vv(&vv).ok()?)))
            .collect()
    }

    /// Acks relayed by another own device: merged like the contact's own (`chat_ack`), which
    /// passes them on to our other devices when they add anything.
    pub(crate) fn chat_apply_acks(&self, acks: &[(String, Vec<u8>)]) -> Result<(), Error> {
        let (me, core) = (self.me()?, self.chat_core()?);
        for (doc, vv) in acks {
            let Some((pair, day)) = parse_doc(doc) else { continue };
            let Some(did) = self.pair_owner(&me, &pair) else { continue };
            let Ok(vv) = decode_vv(vv) else { continue };
            self.chat_ack(&core, &me, &did, &pair, &day, &vv)?;
        }
        Ok(())
    }

    /// Incoming, not deleted messages after `cursor`. Earlier days are all at or before it.
    async fn chat_count_unread(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Me, pair: &str, cursor: i64) -> Result<u32, Error> {
        let from = day_of(cursor - 2 * HOUR_MS);
        let mut n = 0u32;
        for day in core.store.shard_days(pair)? {
            if day < from {
                continue;
            }
            self.chat_shard(core, me, pair, &day).await?;
            let msgs = core.shards.lock().get(&(pair.to_string(), day)).map(|s| s.messages());
            if let Some(Ok(msgs)) = msgs {
                n += msgs.values().filter(|m| m.author != me.id.did() && !m.deleted && m.at > cursor).count() as u32;
            }
        }
        Ok(n)
    }

    /// After a session with an own device came up: fetch what we wanted but could not get, now
    /// that it may be a source.
    pub(crate) fn chat_resume_all(self: &Arc<Self>) {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return };
        for (hash, info) in core.store.wanted_blobs().unwrap_or_default() {
            if let Some(did) = self.pair_owner(&me, &info.pair) {
                self.chat_download(did, hash);
            }
        }
    }
}
