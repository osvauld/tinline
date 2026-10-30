//! Signed DM receipt ledgers across our own devices (`SelfMsg::DmReceipt*` on `tinline/self/5`,
//! negotiated with `cap/dm-receipts/v1`; the session side is `sync-runtime`'s `dm_receipts`).
//!
//! The device that met the contact holds the contact's original Delivered/Read statements; our
//! other devices get the same native ledger ops, never re-signed, and admit them exactly as the
//! contact's own `tinline/chat/6` session would (`MessageReceipts::import_direct`): DID signature
//! under the issuer's writer ID, issuer = that contact or our own account, kinds Delivered/Read,
//! a day we hold. On top: the conversation is one of ours with a contact in good standing (a
//! contact, not blocked either way). Our own statements travel too: another device relays them to
//! the contact while the signing device sleeps, and its pump counts them as account coverage.

use super::receipts::{RECEIPT_BATCH, dm_scope};
use super::*;
use crate::chat::own::{MAX_OWN_DOCS, RECEIPTS_MARK, parse_doc};

/// Provisional: ledger days one `DmReceiptHello` lists (newest shard days first). Older days with
/// receipts still travel when they change while a session is up.
pub const MAX_OWN_RECEIPT_DAYS: usize = 1024;

/// The dirty mark for one day's receipt ledger.
pub fn mark(pair: &str, day: &str) -> String {
    format!("{RECEIPTS_MARK}{}", doc_name(pair, day))
}

/// What one export pass yields for another own device.
pub struct OwnReceiptBatch {
    /// Original native ledger ops beyond the peer's vector (bounded).
    pub update: Vec<u8>,
    /// What the peer holds once it applied `update`.
    pub frontier: VersionVector,
    /// Our whole ledger frontier for that day.
    pub vv: VersionVector,
    /// More remains beyond `frontier`.
    pub more: bool,
}

impl ChatRt {
    /// The contact of `pair` when its receipts may cross the own-device link: a conversation of
    /// ours with a contact neither side blocks. Never "You" (it has no receipts).
    fn own_receipt_contact(&self, me: &Me, pair: &str) -> Option<String> {
        let did = self.pair_owner(me, pair)?;
        (did != me.id.did() && self.chat_contact(&did, true).is_ok()).then_some(did)
    }

    /// `dm/{pair}/{day}` -> `(pair, day, contact)` for a doc whose receipts may cross the link, and
    /// whose day we hold.
    fn own_receipt_doc(
        &self,
        core: &ChatCore,
        me: &Me,
        doc: &str,
    ) -> Result<(String, String, String), Error> {
        let (pair, day) =
            parse_doc(doc).ok_or_else(|| Error::Protocol("bad receipt doc".into()))?;
        let did = self
            .own_receipt_contact(me, &pair)
            .ok_or_else(|| Error::Rejected("not a conversation in good standing".into()))?;
        if core.store.shard_meta(&pair, &day)?.is_none() {
            return Err(Error::Rejected("a day we do not hold".into()));
        }
        Ok((pair, day, did))
    }

    /// `(doc, ledger vv)` of every held day with committed receipts, newest day first, at most
    /// `MAX_OWN_RECEIPT_DAYS`. Reads each ledger's stored frontier only (no ledger is opened).
    pub fn chat_own_receipt_days(&self) -> Vec<(String, Vec<u8>)> {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else {
            return vec![];
        };
        let Ok(all) = core.store.all_shards(MAX_OWN_DOCS) else {
            return vec![];
        };
        let mut owners: HashMap<String, bool> = HashMap::new();
        let mut out = Vec::new();
        for (pair, day, _) in all {
            if out.len() >= MAX_OWN_RECEIPT_DAYS {
                break;
            }
            if !*owners
                .entry(pair.clone())
                .or_insert_with(|| self.own_receipt_contact(&me, &pair).is_some())
            {
                continue;
            }
            match core.receipts.stored_vv(&dm_scope(&pair, &day)) {
                Ok(Some(vv)) if !vv.is_empty() => out.push((doc_name(&pair, &day), vv.encode())),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("own-device receipts: ledger state of {day} unreadable: {e}")
                }
            }
        }
        out
    }

    /// Our ledger frontier of `doc`, without opening it; empty for a doc that is not ours.
    pub fn chat_own_receipt_vv(&self, doc: &str) -> VersionVector {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else {
            return VersionVector::default();
        };
        match self.own_receipt_doc(&core, &me, doc) {
            Ok((pair, day, _)) => core
                .receipts
                .stored_vv(&dm_scope(&pair, &day))
                .ok()
                .flatten()
                .unwrap_or_default(),
            Err(_) => VersionVector::default(),
        }
    }

    /// What an own device holding `theirs` of `doc` lacks of our receipt ledger (one bounded
    /// batch); `None` when it lacks nothing or the doc may not cross the link.
    pub fn chat_own_receipt_export(
        &self,
        doc: &str,
        theirs: &VersionVector,
    ) -> Result<Option<OwnReceiptBatch>, Error> {
        let (me, core) = (self.me()?, self.chat_core()?);
        let Ok((pair, day, _)) = self.own_receipt_doc(&core, &me, doc) else {
            return Ok(None);
        };
        let scope = dm_scope(&pair, &day);
        let Some(vv) = core.receipts.stored_vv(&scope)? else {
            return Ok(None);
        };
        if covers(theirs, &vv) {
            return Ok(None);
        }
        let Some(batch) = core.receipts.export(&scope, theirs, RECEIPT_BATCH)? else {
            return Ok(None);
        };
        Ok(Some(OwnReceiptBatch {
            update: batch.update,
            frontier: batch.frontier,
            vv,
            more: batch.more,
        }))
    }

    /// Imports receipt-ledger ops of `doc` relayed by an own device, under the contact session's
    /// admission plus the own-device rules (module doc). Returns our frontier afterwards. `Err`:
    /// refused, nothing stored (the caller logs it; the session goes on). New statements are
    /// passed on to our other devices and redraw our affected messages.
    pub async fn chat_own_receipts_apply(
        self: &Arc<Self>,
        doc: &str,
        update: &[u8],
    ) -> Result<VersionVector, Error> {
        let (me, core) = (self.me()?, self.chat_core()?);
        let (pair, day, did) = self.own_receipt_doc(&core, &me, doc)?;
        let scope = dm_scope(&pair, &day);
        let fresh = core.receipts.import_direct(&scope, &did, update)?;
        if !fresh.is_empty() {
            self.selfsync_dirty(&mark(&pair, &day));
            self.chat_receipts_changed(&core, &me, &did, &pair, &day, &fresh)
                .await;
        }
        Ok(core.receipts.vv(&scope)?)
    }
}

/// `local` holds at least every op `advertised` names.
pub fn covers(local: &VersionVector, advertised: &VersionVector) -> bool {
    advertised
        .iter()
        .all(|(p, n)| local.get(p).copied().unwrap_or(0) >= *n)
}
