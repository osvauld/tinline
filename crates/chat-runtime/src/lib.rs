//! The chat runtime: 1:1 chat (sealed store, shard cache, sessions over `tinline/chat/3`,
//! outbox and retry, encrypted blob transfers, presence, receipts, own-device chat sync) and
//! account profiles (profile docs, pictures over the chat blob hub). No UniFFI.
//!
//! [`ChatRt`] holds this runtime's state and derefs to the node's [`NodeCtx`], so the engine reads
//! the account session, the endpoint and the contacts cluster directly. p2pcore builds one per
//! node, declares the records in [`chat::api`] to UniFFI and forwards [`ChatSink`] and
//! [`ProfileSink`] to the app's callbacks. The search index ([`SearchIx`]) is shared with the group
//! engine; reconcile on open is p2pcore's (it reads both engines), so [`ChatRt::chat_open`] says
//! whether it opened the store.
//!
//! Locks: the `core` slot and the sinks are taken for short, synchronous stretches and never
//! across an `.await`; `NodeCtx`'s lock order applies (`node_runtime` crate docs). Everything
//! `pub` here beyond `ChatRt`'s constructor and the records is for p2pcore's facade and the other
//! engines, not a stable API.

use std::collections::HashSet;
use std::ops::Deref;
use std::sync::Arc;

use node_runtime::NodeCtx;
use parking_lot::Mutex;
use search_ix::SearchIx;
use tokio::sync::Semaphore;

pub use node_runtime::Error;

pub mod chat;
pub mod profile;

pub use chat::api::ChatSink;
pub use profile::ProfileSink;

pub struct ChatRt {
    ctx: Arc<NodeCtx>,
    /// The open chat store and its sessions; `None` while locked or between accounts.
    pub core: Mutex<Option<Arc<chat::engine::ChatCore>>>,
    pub chat_events: Mutex<Option<Arc<dyn ChatSink>>>,
    /// The selected account's search index (`search.redb`), open while the chat store is.
    pub search: Arc<SearchIx>,
    pub profile_events: Mutex<Option<Arc<dyn ProfileSink>>>,
    /// Picture downloads in flight: `(epoch, did, hash)`.
    pub profile_downloads: Mutex<HashSet<(u64, String, String)>>,
    pub profile_fetches: Arc<Semaphore>,
}

impl ChatRt {
    pub fn new(ctx: Arc<NodeCtx>, search: Arc<SearchIx>) -> Self {
        Self {
            ctx,
            core: Mutex::new(None),
            chat_events: Mutex::new(None),
            search,
            profile_events: Default::default(),
            profile_downloads: Default::default(),
            profile_fetches: Arc::new(Semaphore::new(4)),
        }
    }

    pub fn set_chat_sink(&self, sink: Arc<dyn ChatSink>) {
        *self.chat_events.lock() = Some(sink);
    }

    pub fn set_profile_sink(&self, sink: Arc<dyn ProfileSink>) {
        *self.profile_events.lock() = Some(sink);
    }
}

impl Deref for ChatRt {
    type Target = NodeCtx;

    fn deref(&self) -> &NodeCtx {
        &self.ctx
    }
}
