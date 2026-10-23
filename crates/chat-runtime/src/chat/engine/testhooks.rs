//! Test hooks over the chat engine's state, for p2pcore's `*_for_test` `Node` methods
//! (`testkit` only).

use super::*;
use crate::chat::doc::{MsgRec, Shard, Writer, day_of, doc_name, new_device_value, pair_id};

impl ChatRt {
    /// Test hook (`forged_own_batch_for_test`): offers a hand-made batch for today's shard with
    /// `contact` to the validator that judges batches relayed by our other devices. `kind`: 0 = a
    /// message of the contact's signed by another account's DID key, 1 = a message of ours signed
    /// by another account's DID key, 2 = a stranger's message, validly signed by the stranger, 3 =
    /// a genuine message of ours (control). Each is written under the writer ID its author and
    /// device ID give, so only the signature (or the author) is wrong.
    pub async fn chat_forged_own_batch_for_test(self: &Arc<Self>, contact: String, kind: u8) -> Result<(), Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let pair = pair_id(me.id.did(), &contact);
        let at = node_runtime::now_ms() as i64;
        let day = day_of(at);
        let (stranger, _) = identity::generate();
        let (author, writer) = match kind {
            0 => (contact.clone(), Writer::forging(stranger, new_device_value(), &contact)),
            1 => (me.id.did().to_string(), Writer::forging(stranger, new_device_value(), me.id.did())),
            2 => (stranger.did().to_string(), Writer::new(stranger, new_device_value())),
            _ => (me.id.did().to_string(), self.chat_writer(&me, &core)),
        };
        let shard = Shard::new(&pair, &day, writer.writer_id());
        let rec = MsgRec { id: format!("forged{kind}"), author, at, text: format!("forged {kind}"), edited_at: None, deleted: false, reply_to: None, file: None, dev: [0; 32], did_sig: Vec::new() };
        let update = shard.add_message(&rec, &writer).map_err(|e| Error::Io(e.to_string()))?;
        let doc = doc_name(&pair, &day);
        self.chat_own_apply(&doc, &update).await
    }

    /// Test hook: does the chat store hold any shard of the conversation between `me_did` and
    /// `peer_did` (`me_did == peer_did`: the "You" docs)?
    pub fn chat_has_docs_for_test(&self, me_did: &str, peer_did: &str) -> bool {
        let Ok(core) = self.chat_core() else { return false };
        let pair = pair_id(me_did, peer_did);
        core.store.shard_days(&pair).map(|d| !d.is_empty()).unwrap_or(false)
    }

    /// Test hook (P9): our writer ID (`None` while the chat store is not open).
    pub fn chat_writer_id_for_test(&self) -> Option<u64> {
        self.chat_core().ok().map(|c| c.wid)
    }

    /// Test hook (P9): every `(day, Loro peer id, op count)` in our copy of the conversation with
    /// `peer_did`, and per message `(id, peer id of the change that created it)`.
    #[allow(clippy::type_complexity)]
    pub async fn chat_doc_peers_for_test(self: &Arc<Self>, peer_did: String) -> (Vec<(String, u64, i32)>, Vec<(String, u64)>) {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return (vec![], vec![]) };
        let pair = pair_id(me.id.did(), &peer_did);
        let (mut peers, mut created) = (Vec::new(), Vec::new());
        for day in core.store.shard_days(&pair).unwrap_or_default() {
            let Ok(shards) = self.chat_shard(&core, &pair, &day).await else { continue };
            let Some(s) = shards.get(&(pair.clone(), day.clone())) else { continue };
            peers.extend(s.vv().iter().map(|(p, c)| (day.clone(), *p, *c)));
            for id in s.messages().map(|m| m.into_keys().collect::<Vec<_>>()).unwrap_or_default() {
                if let Some((p, _)) = s.creator_end(&id) {
                    created.push((id, p));
                }
            }
        }
        (peers, created)
    }

    /// Test hook (P10): the next `n` chat writes (`chat_persist`) fail, as if the disk were full.
    pub fn chat_fail_next_writes_for_test(&self, n: u32) {
        if let Ok(core) = self.chat_core() {
            core.fail_writes.store(n, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Test hook: messages written now are stamped `ms` ahead of the real clock (negative: a
    /// message written at that past time by a right clock; edits, hello days and compaction use it
    /// too). The live window and catch-up decision keep the real clock.
    pub fn chat_clock_offset_for_test(&self, ms: i64) {
        if let Ok(core) = self.chat_core() {
            core.clock_wrong.store(false, std::sync::atomic::Ordering::Relaxed);
            core.clock_ms.store(ms, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Test hook: this device's clock is wrong by `ms`: as `chat_clock_offset_for_test`, and the
    /// live window and catch-up decision use it as well (0 sets it right again).
    pub fn chat_clock_skew_for_test(&self, ms: i64) {
        if let Ok(core) = self.chat_core() {
            core.clock_ms.store(ms, std::sync::atomic::Ordering::Relaxed);
            core.clock_wrong.store(ms != 0, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
