//! Test-only: writes 1:1 messages of any author at an injected time through the real persist
//! path (`chat_persist`, so the search hook runs), for the year-scale measurements
//! (`docs/scale-year.md`). Not used by the product.

use std::sync::Arc;

use super::*;

impl ChatRt {
    /// One message in the conversation with contact `with`, authored by us or by that contact
    /// (a registered synthetic peer: `peer` holds its identity and device), at unix ms `at`.
    /// Stored as a delivered message (no outbox mark, no network). Returns the message id.
    pub async fn synth_dm(self: &Arc<Self>, with: &str, author: &str, peer: Option<(identity::Identity, [u8; 32])>, text: String, at: i64, file: Option<FileRef>) -> Result<String, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(with, true)?;
        let pair = pair_id(me.id.did(), with);
        let day = day_of(at);
        let mut shards = self.chat_shard(&core, &pair, &day).await?;
        let mine = author == me.id.did();
        let writer = if mine {
            self.chat_writer(&me, &core)
        } else {
            let (id, dev) = peer.ok_or(Error::NotFound)?;
            Writer::new(id, dev)
        };
        let rec = MsgRec { id: random_msg_id(), author: author.to_string(), at, text, edited_at: None, deleted: false, reply_to: None, file, dev: [0; 32], did_sig: Vec::new() };
        let shard = shards.get_mut(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
        let update = shard.add_message(&rec, &writer)?;
        let mut ops = vec![core.store.mi_op(&pair, &rec.id, &day)?];
        if mine {
            ops.push(core.store.put_mo_op(&pair, &day, &rec.id, shard.peer(), shard.my_counter())?);
        }
        let (fo, _) = self.chat_register_file(&core, &pair, &day, &rec, false)?;
        ops.extend(fo);
        let (cops, _) = self.chat_conv_ops(&core, &me, with, &pair, &[(&rec, true)])?;
        ops.extend(cops);
        self.chat_persist(&core, &pair, shard, &update, ops, &[&rec])?;
        Ok(rec.id)
    }
}
