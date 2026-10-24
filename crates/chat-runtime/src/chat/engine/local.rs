//! Local writes: contacts, send, edit.

use super::*;

/// Shown as is when a message (new or edited) is over the limit.
const MSG_TOO_LONG: &str = "message too long";

impl ChatRt {
    // ---- local writes ----------------------------------------------------------------------------

    /// `did` is us: the "You" conversation (saved items; it exists only on our own devices).
    pub fn is_self_did(&self, did: &str) -> bool {
        self.shared.lock().me.as_ref().is_some_and(|m| m.id.did() == did)
    }

    /// Whose conversation we may read: a contact, ourselves (the "You" conversation), or someone
    /// whose notice we hold (they blocked us and their contact entry went, U20; the history
    /// stays, read-only). With `write`, only a contact that neither side blocks, or ourselves.
    pub(super) fn chat_contact(&self, did: &str, write: bool) -> Result<(), Error> {
        let s = self.shared.lock();
        if s.me.as_ref().is_some_and(|m| m.id.did() == did) {
            return Ok(());
        }
        let contact = s.state.contacts.iter().any(|c| c.did == did);
        if !write {
            return if contact || s.state.notices.contains_key(did) { Ok(()) } else { Err(Error::NotFound) };
        }
        if node_runtime::block::blocked_me(&s.state, did) {
            return Err(Error::Rejected(node_runtime::BLOCKED.into()));
        }
        if s.state.blocked.contains(did) {
            return Err(Error::Protocol("you blocked them; unblock first".into()));
        }
        if contact { Ok(()) } else { Err(Error::NotFound) }
    }

    /// Adds a message of ours to today's shard, persists it, tells the session (or dials).
    pub async fn chat_send_message(
        self: &Arc<Self>,
        did: &str,
        text: String,
        reply_to: Option<String>,
        file: Option<FileRef>,
    ) -> Result<Message, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did, true)?;
        if text.len() > MAX_TEXT {
            return Err(Error::Protocol(MSG_TOO_LONG.into()));
        }
        let pair = pair_id(me.id.did(), did);
        let at = self.chat_now();
        let day = day_of(at);
        let held = self.chat_shard(&core, &pair, &day).await?;
        let rec = MsgRec { id: random_msg_id(), author: me.id.did().to_string(), at, text, edited_at: None, deleted: false, reply_to, file, dev: core.device_id, did_sig: Vec::new() };
        let conv = {
            let mut shards = held;
            let shard = shards.get_mut(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
            let update = shard.add_message(&rec, &self.chat_writer(&me, &core))?;
            // The tick marker names the peer id it was written under: our writer ID.
            let mut ops = vec![core.store.put_mo_op(&pair, &day, &rec.id, shard.peer(), shard.my_counter())?, core.store.mi_op(&pair, &rec.id, &day)?];
            if !self.is_self_did(did) {
                ops.push(core.store.out_op(&pair, &day, true)?);
            }
            let (fo, _) = self.chat_register_file(&core, &pair, &day, &rec, false)?;
            ops.extend(fo);
            let (cops, conv) = self.chat_conv_ops(&core, &me, did, &pair, &[(&rec, true)])?;
            ops.extend(cops);
            self.chat_persist(&core, &pair, shard, &update, ops, &[&rec])?;
            conv
        };
        let m = self.chat_api_message(&core, &me, did, &pair, &day, &rec);
        self.chat_after_local_write(&core, did, &day);
        if let Some(ev) = self.ev() {
            ev.on_message_added(m.clone());
            if let Some(c) = self.chat_api_chat(&core, &me, &conv) {
                ev.on_chat_changed(c);
            }
        }
        Ok(m)
    }

    /// Test hook (`write_as_sibling_for_test`): a message of ours sent at `at`, under a fresh
    /// device ID's writer ID, stored like one relayed by an own device (pending, not pushed).
    pub async fn chat_write_as_sibling_for_test(self: &Arc<Self>, did: &str, text: String, at: i64) -> Result<String, Error> {
        self.chat_write_in_day_for_test(did, text, at, &day_of(at)).await
    }

    /// As `chat_write_as_sibling_for_test`, but filed in the shard of `day` whatever `at` is
    /// (a message a receiver must judge against that day's window).
    pub async fn chat_write_in_day_for_test(self: &Arc<Self>, did: &str, text: String, at: i64, day: &str) -> Result<String, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did, true)?;
        let pair = pair_id(me.id.did(), did);
        let day = day.to_string();
        let mut shards = self.chat_shard(&core, &pair, &day).await?;
        let dev = new_device_value();
        let w = Writer::new(me.id.clone(), dev);
        let rec = MsgRec { id: random_msg_id(), author: me.id.did().to_string(), at, text, edited_at: None, deleted: false, reply_to: None, file: None, dev, did_sig: Vec::new() };
        let key = (pair.clone(), day.clone());
        let shard = shards.get_mut(&key).ok_or(Error::NotFound)?;
        let update = shard.add_message(&rec, &w)?;
        let end = shard.vv().get(&w.writer_id()).copied().unwrap_or(0);
        let mut ops = vec![core.store.put_mo_op(&pair, &day, &rec.id, w.writer_id(), end)?, core.store.mi_op(&pair, &rec.id, &day)?, core.store.out_op(&pair, &day, true)?];
        let (cops, _) = self.chat_conv_ops(&core, &me, did, &pair, &[(&rec, true)])?;
        ops.extend(cops);
        let r = self.chat_persist(&core, &pair, shard, &update, ops, &[&rec]);
        // The cached doc now writes under the sibling's writer ID: load it again for ours.
        shards.remove(&key);
        r.map(|_| rec.id)
    }

    /// How this install writes chat messages: signed by the account's DID key, under the writer
    /// ID of this install's device ID (P9c: the same for every conversation).
    pub fn chat_writer(&self, me: &Me, core: &ChatCore) -> Writer {
        Writer::new(me.id.clone(), core.device_id)
    }

    pub(super) fn chat_after_local_write(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str, day: &str) {
        if self.is_self_did(did) {
            return;
        }
        // Writing is local activity: the session stays open and carries our presence.
        core.mark_active(did, None);
        self.chat_wake(core, did);
        for s in self.chat_sessions_of(core, did) {
            let _ = s.tx.send(Cmd::Push(day.to_string()));
        }
    }

    pub async fn chat_edit(self: &Arc<Self>, did: &str, id: &str, text: Option<String>) -> Result<Message, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did, true)?;
        let pair = pair_id(me.id.did(), did);
        let day = core.store.mi(&pair, id)?.ok_or(Error::NotFound)?;
        let held = self.chat_shard(&core, &pair, &day).await?;
        let (rec, conv) = {
            let mut shards = held;
            let shard = shards.get_mut(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
            let cur = shard.messages()?.remove(id).ok_or(Error::NotFound)?;
            if cur.author != me.id.did() {
                return Err(Error::Protocol("only the author can change a message".into()));
            }
            let update = match &text {
                Some(t) => {
                    if t.len() > MAX_TEXT {
                        return Err(Error::Protocol(MSG_TOO_LONG.into()));
                    }
                    shard.edit(id, t, self.chat_now(), &self.chat_writer(&me, &core))
                }
                None => shard.delete(id, self.chat_now(), &self.chat_writer(&me, &core)),
            }
            ?;
            let rec = shard.messages()?.remove(id).ok_or(Error::NotFound)?;
            let ops = if self.is_self_did(did) { vec![] } else { vec![core.store.out_op(&pair, &day, true)?] };
            let (cops, conv) = self.chat_conv_ops(&core, &me, did, &pair, &[(&rec, false)])?;
            let mut ops = ops;
            ops.extend(cops);
            self.chat_persist(&core, &pair, shard, &update, ops, &[&rec])?;
            (rec, conv)
        };
        let m = self.chat_api_message(&core, &me, did, &pair, &day, &rec);
        self.chat_after_local_write(&core, did, &day);
        if let Some(ev) = self.ev() {
            ev.on_message_changed(m.clone());
            if let Some(c) = self.chat_api_chat(&core, &me, &conv) {
                ev.on_chat_changed(c);
            }
        }
        Ok(m)
    }
}
