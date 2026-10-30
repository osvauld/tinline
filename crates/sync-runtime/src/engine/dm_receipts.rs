//! Signed DM receipt ledgers between own devices (negotiated `cap/dm-receipts/v1` on self/5).
//! After both `Hello`s each side sends `DmReceiptHello` (its ledger frontier per held day), then
//! `DmReceipts` with the original ops the other lacks; a growing ledger is pushed live through the
//! `!receipts/{doc}` dirty mark. Admission is the chat runtime's (`engine::own_receipts`): a
//! refused frame is logged and dropped, the session goes on. A peer without the marker gets none
//! of these frames (it could not decode them).
use super::*;
use chat_runtime::chat::engine::own_receipts::MAX_OWN_RECEIPT_DAYS;
use chat_runtime::chat::own::RECEIPTS_MARK;

pub const CAPABILITY: &str = "cap/dm-receipts/v1";
/// Provisional: frames one send pass writes for a day; the rest follows the next trigger.
pub const FRAMES_PER_SEND: usize = 16;

#[derive(Default)]
pub struct DmReceipts {
    /// The peer's `Hello` carried the marker.
    pub enabled: bool,
    /// Our `DmReceiptHello` went out (nothing is pushed before it).
    pub sent_hello: bool,
    /// What the peer is known to hold per doc (its hello, acks, and what we sent).
    pub known: HashMap<String, VersionVector>,
}

pub fn supported(docs: &[(String, Vec<u8>)]) -> bool {
    docs.iter().any(|(name, vv)| {
        name == CAPABILITY
            && VersionVector::decode(vv)
                .ok()
                .is_some_and(|v| v == VersionVector::default())
    })
}

/// The dirty mark `!receipts/{doc}` -> `doc`.
pub fn marked(name: &str) -> Option<&str> {
    name.strip_prefix(RECEIPTS_MARK)
}

fn doc_ok(doc: &str) -> Result<(), Error> {
    parse_doc(doc)
        .map(|_| ())
        .ok_or_else(|| Error::Protocol("bad receipt doc".into()))
}

impl DmReceipts {
    /// Our `DmReceiptHello` is owed: negotiated, not sent yet.
    pub fn hello_due(&self) -> bool {
        self.enabled && !self.sent_hello
    }

    /// Ledger ops may be pushed: negotiated and our hello is out. Never for an older peer.
    pub fn may_push(&self) -> bool {
        self.enabled && self.sent_hello
    }

    /// Our hello, once per session, after the peer's `Hello` negotiated the marker.
    pub async fn start(
        &mut self,
        inner: &Arc<SyncRt>,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        if !self.hello_due() {
            return Ok(());
        }
        self.sent_hello = true;
        writer
            .send(&SelfMsg::DmReceiptHello {
                days: inner.chat.chat_own_receipt_days(),
            })
            .await
    }

    /// Sends what the peer lacks of one day's ledger (bounded).
    pub async fn push(
        &mut self,
        inner: &Arc<SyncRt>,
        doc: &str,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        if !self.may_push() {
            return Ok(());
        }
        for _ in 0..FRAMES_PER_SEND {
            let theirs = self.known.get(doc).cloned().unwrap_or_default();
            let batch = match inner.chat.chat_own_receipt_export(doc, &theirs) {
                Ok(Some(b)) => b,
                Ok(None) => return Ok(()),
                Err(e) => {
                    tracing::debug!("own-device receipt export of {doc}: {e}");
                    return Ok(());
                }
            };
            writer
                .send(&SelfMsg::DmReceipts {
                    doc: doc.to_string(),
                    vv: batch.vv.encode(),
                    update: batch.update,
                })
                .await?;
            self.known
                .entry(doc.to_string())
                .or_default()
                .merge(&batch.frontier);
            if !batch.more {
                break;
            }
        }
        Ok(())
    }
}

pub async fn handle(
    inner: &Arc<SyncRt>,
    peer: &mut PeerChat,
    message: SelfMsg,
    writer: &mut FrameWriter,
) -> Result<(), Error> {
    let state = &mut peer.dm_receipts;
    if !state.enabled {
        return Err(Error::Protocol("dm receipts were not negotiated".into()));
    }
    match message {
        SelfMsg::DmReceiptHello { days } => {
            if days.len() > MAX_OWN_RECEIPT_DAYS {
                return Err(Error::Protocol("too many receipt days".into()));
            }
            for (doc, vv) in days {
                doc_ok(&doc)?;
                state.known.insert(doc, group_history::vv(&vv)?);
            }
            state.start(inner, writer).await?;
            for (doc, _) in inner.chat.chat_own_receipt_days() {
                state.push(inner, &doc, writer).await?;
            }
        }
        SelfMsg::DmReceipts { doc, vv, update } => {
            doc_ok(&doc)?;
            let theirs = group_history::vv(&vv)?;
            state.known.entry(doc.clone()).or_default().merge(&theirs);
            let ours = match inner.chat.chat_own_receipts_apply(&doc, &update).await {
                Ok(v) => v,
                Err(e) => {
                    // Not stored, not projected; the own-device session goes on.
                    inner.log(format!("rejected own-device receipts of {doc}: {e}"));
                    inner.chat.chat_own_receipt_vv(&doc)
                }
            };
            writer
                .send(&SelfMsg::DmReceiptAck {
                    doc,
                    vv: ours.encode(),
                })
                .await?;
        }
        SelfMsg::DmReceiptAck { doc, vv } => {
            doc_ok(&doc)?;
            state.known.insert(doc, group_history::vv(&vv)?);
        }
        _ => return Err(Error::Protocol("unexpected receipt message".into())),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
