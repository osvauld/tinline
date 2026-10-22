//! Flushing the account state and call log into the account doc and call doc of own-device sync
//! (`account_model::sync`) after a local write. The sessions that ship the docs are the sync
//! engine's; this only keeps the docs current and says so through the signal.

use account_model::sync::accdoc::Ctx;

use super::*;

/// Where the account doc is sealed in the account store.
pub const DOC_KEY: &str = "sync/account";
/// Where the call doc is sealed in the account store.
pub const CALLS_KEY: &str = "sync/calls";

/// The context the doc merge rules need from the node.
pub fn own_ctx(me: &Me) -> Ctx<'_> {
    Ctx { my_did: me.id.did(), me_device: me.device, now_ms: now_ms(), relay_hint }
}

impl NodeCtx {
    /// Seals the account doc and call doc into the account store.
    pub fn save_doc(&self) {
        let (acc, calls, backing) = {
            let s = self.shared.lock();
            (s.acc.clone(), s.calls.clone(), s.acct.as_ref().and_then(|a| a.backing.clone()))
        };
        let Some(Backing::Sealed(db)) = backing else {
            return;
        };
        let docs = [(DOC_KEY, acc.as_deref().map(|d| &**d)), (CALLS_KEY, calls.as_deref().map(|d| &**d))];
        for (key, doc) in docs {
            if let Some(doc) = doc
                && let Err(e) = db.put(key, &doc.snapshot())
            {
                tracing::warn!("saving {key}: {e}");
            }
        }
    }

    /// A local write happened (state or call log): flush it into the doc and tell the sessions.
    pub fn sync_local(&self, epoch: Option<u64>) {
        let Ok(me) = self.me() else { return };
        let changed = {
            let s = self.shared.lock();
            if epoch.is_some_and(|e| e != s.epoch) || s.epoch != me.epoch {
                return;
            }
            let (Some(acc), Some(calls)) = (s.acc.clone(), s.calls.clone()) else {
                return;
            };
            let a = acc.write_from(&s.state, &own_ctx(&me)).unwrap_or_else(|e| {
                tracing::warn!("account doc flush: {e}");
                false
            });
            let c = calls.write_from(&s.history.snapshot()).unwrap_or_else(|e| {
                tracing::warn!("calls doc flush: {e}");
                false
            });
            a || c
        };
        if changed {
            self.save_doc();
            self.sync_bump();
        }
    }
}
