//! Reading: API projections, lists, day pages, read marks.

use super::*;

impl Inner {
    // ---- reading -----------------------------------------------------------------------------------

    pub(super) fn chat_attachment(&self, core: &ChatCore, f: &FileRef, outgoing: bool) -> Attachment {
        let info = core.store.blob(&f.hash).ok().flatten();
        let dl = core.downloading.lock().get(&f.hash).map(|d| (d.done, d.total));
        // Our own message sent from another of our devices has no bytes here until fetched.
        let (state, transferred) = match (&info, dl, outgoing) {
            (None, _, true) => (TransferState::Ready, 0),
            (_, Some((done, _)), _) => (TransferState::Downloading, done),
            (Some(i), _, _) if i.ready => (TransferState::Ready, 0),
            (Some(i), _, _) if i.failed => (TransferState::Failed, 0),
            _ => (TransferState::Remote, 0),
        };
        Attachment {
            hash: f.hash.clone(),
            name: f.name.clone(),
            size: f.size,
            mime: f.mime.clone(),
            kind: if f.kind == "voice" { AttachmentKind::Voice } else { AttachmentKind::File },
            duration_ms: f.duration_ms,
            waveform: f.waveform.clone(),
            width: f.width,
            height: f.height,
            preview: hex::decode(&f.preview).ok().filter(|p| p.len() == crate::chat::photo::PREVIEW_LEN).unwrap_or_default(),
            state,
            transferred,
        }
    }

    /// Two ticks once the contact holds the change that created our message (`mo`: the writer
    /// ID it was created under, here or on another of our devices, and the op count after it).
    pub(super) fn chat_delivery(core: &ChatCore, pair: &str, day: &str, id: &str) -> DeliveryState {
        let Some((peer, c)) = core.store.mo(pair, day, id).ok().flatten() else { return DeliveryState::Delivered };
        let ack = core.store.ack(pair, day).ok().flatten().and_then(|b| decode_vv(&b).ok()).and_then(|v| v.get(&peer).copied()).unwrap_or(0);
        if c > ack { DeliveryState::Pending } else { DeliveryState::Delivered }
    }

    pub(crate) fn chat_api_message(&self, core: &ChatCore, me: &Me, did: &str, pair: &str, day: &str, rec: &MsgRec) -> Message {
        let outgoing = rec.author == me.id.did();
        // "You" items go nowhere; a message written on another of our devices has no counter
        // here and shows as held (it reached this device).
        let delivery = if outgoing && !self.is_self_did(did) { Self::chat_delivery(core, pair, day, &rec.id) } else { DeliveryState::Delivered };
        Message {
            id: rec.id.clone(),
            peer_did: did.to_string(),
            author_did: rec.author.clone(),
            outgoing,
            at: rec.at.max(0) as u64,
            text: rec.text.clone(),
            edited_at: rec.edited_at.map(|t| t.max(0) as u64),
            deleted: rec.deleted,
            reply_to: rec.reply_to.clone(),
            attachment: rec.file.as_ref().map(|f| self.chat_attachment(core, f, outgoing)),
            delivery,
        }
    }

    pub(crate) fn chat_api_chat(&self, core: &ChatCore, me: &Me, conv: &ConvMeta) -> Option<Chat> {
        let is_self = conv.peer_did == me.id.did();
        let name = if is_self {
            "You".to_string()
        } else {
            let s = self.shared.lock();
            crate::block::known_name(&s.state, &conv.peer_did)?
        };
        let pair = pair_id(me.id.did(), &conv.peer_did);
        let (last_outgoing, last_delivery, last_activity, preview) = match &conv.last {
            Some(l) => {
                let delivery = match core.store.mi(&pair, &l.id).ok().flatten() {
                    Some(day) if l.outgoing && !is_self => Self::chat_delivery(core, &pair, &day, &l.id),
                    _ => DeliveryState::Delivered,
                };
                (l.outgoing, delivery, l.at.max(0) as u64, l.preview.clone())
            }
            None => (false, DeliveryState::Delivered, 0, String::new()),
        };
        Some(Chat { peer_did: conv.peer_did.clone(), peer_name: name, preview, last_outgoing, last_delivery, last_activity, unread: conv.unread })
    }

    pub(crate) fn chat_list(&self) -> Result<Vec<Chat>, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let mut dids: Vec<String> = {
            let s = self.shared.lock();
            let mut v: Vec<String> = s.state.contacts.iter().map(|c| c.did.clone()).collect();
            // Kept, read-only conversations of people whose block deleted their contact entry.
            v.extend(s.state.notices.keys().filter(|d| !s.state.contacts.iter().any(|c| &c.did == *d)).cloned());
            v
        };
        dids.push(me.id.did().to_string());
        let mut out = Vec::new();
        for did in dids {
            let pair = pair_id(me.id.did(), &did);
            let conv = core.store.conv(&pair)?.unwrap_or_else(|| ConvMeta { peer_did: did.clone(), ..Default::default() });
            if let Some(c) = self.chat_api_chat(&core, &me, &conv) {
                out.push(c);
            }
        }
        out.sort_by(|a, b| b.last_activity.cmp(&a.last_activity).then_with(|| a.peer_name.cmp(&b.peer_name)));
        Ok(out)
    }

    pub(crate) async fn chat_day(self: &Arc<Self>, did: &str, day: Option<String>) -> Result<DayPage, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did, false)?;
        let pair = pair_id(me.id.did(), did);
        let days = core.store.shard_days(&pair)?;
        let mut with_msgs = Vec::new();
        for d in &days {
            if core.store.shard_meta(&pair, d)?.is_some_and(|m| m.n_messages > 0) {
                with_msgs.push(d.clone());
            }
        }
        let day = match day {
            Some(d) => {
                if !day_valid(&d) {
                    return Err(Error::Protocol("bad day".into()));
                }
                d
            }
            None => match with_msgs.first() {
                Some(d) => d.clone(),
                None => return Ok(DayPage { day: day_of(now_ms()), messages: vec![], older_day: None }),
            },
        };
        let older_day = with_msgs.iter().find(|d| **d < day).cloned();
        if core.store.shard_meta(&pair, &day)?.is_none() {
            return Ok(DayPage { day, messages: vec![], older_day });
        }
        let recs = {
            let shards = self.chat_shard(&core, &pair, &day).await?;
            shards.get(&(pair.clone(), day.clone())).map(|s| s.messages()).transpose().map_err(|e| Error::Io(e.to_string()))?.unwrap_or_default()
        };
        let mut recs: Vec<MsgRec> = recs.into_values().collect();
        recs.sort_by(|a, b| a.order().cmp(&b.order()));
        let messages = recs.iter().map(|r| self.chat_api_message(&core, &me, did, &pair, &day, r)).collect();
        Ok(DayPage { day, messages, older_day })
    }

    /// Everything incoming up to now is read: unread goes to 0 and the cursor moves to the newest
    /// incoming message, which our other devices pick up (their unread clears too).
    pub(crate) fn chat_mark_read(&self, did: &str) -> Result<(), Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let pair = pair_id(me.id.did(), did);
        if let Some(mut conv) = core.store.conv(&pair)?
            && (conv.unread > 0 || conv.read_upto < conv.last_in)
        {
            conv.unread = 0;
            conv.read_upto = conv.read_upto.max(conv.last_in);
            core.store.db.apply(&[core.store.conv_op(&pair, &conv)?])?;
            self.selfsync_dirty(READ_MARK);
            if let (Some(ev), Some(c)) = (self.ev(), self.chat_api_chat(&core, &me, &conv)) {
                ev.on_chat_changed(c);
            }
        }
        Ok(())
    }
}
