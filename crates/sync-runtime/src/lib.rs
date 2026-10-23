//! The own-device runtime: linking a new install (`tinline/link/1`) and own-device sync
//! (`tinline/self/3`) of the account doc, call history, chat shards, group lists, group contents,
//! history and receipts. See docs/design/device-linking.md and docs/protocol.md. No UniFFI.
//!
//! [`SyncRt`] holds this runtime's state and derefs to the node's [`NodeCtx`]. It sits above the
//! chat and group runtimes and calls them directly; they reach it only through `NodeCtx`'s
//! `OwnDocSignal` (`selfsync_dirty`, `sync_bump`, `sync_kick`), which [`engine::SelfSync`] backs
//! as the `OwnDocSink`. p2pcore builds one per node and forwards [`LinkSink`] to the app's
//! `LinkEvents`. Everything `pub` here beyond `SyncRt`'s constructor is for p2pcore's facade and
//! tests, not a stable API.

use std::ops::Deref;
use std::sync::Arc;

use chat_runtime::ChatRt;
use group_runtime::GroupRt;
use node_runtime::NodeCtx;
use parking_lot::Mutex;

pub use node_runtime::Error;

// The own-device docs and their merge rules are `account_model::sync`.
pub use account_model::sync::{accdoc, callsdoc, profilecache, profiledoc};
/// The own-device doc wrapper lives in the `crdt` crate.
pub mod doc {
    pub use crdt::doc::Doc;
}
pub mod devices_api;
pub use devices_api::DeviceEntry;
pub mod engine;
pub mod frames;
pub mod link;

/// Progress of linking and of own-device sync, for the app (p2pcore's `LinkEvents`). Calls
/// arrive on a core thread.
pub trait LinkSink: Send + Sync {
    fn on_link_code(&self, code: String, peer_label: String);
    fn on_link_done(&self, did: String, name: String);
    fn on_link_failed(&self, reason: String);
    fn on_devices_changed(&self);
    fn on_history_changed(&self);
    fn on_unlinked(&self, did: String);
}

pub struct SyncRt {
    ctx: Arc<NodeCtx>,
    pub chat: Arc<ChatRt>,
    pub group: Arc<GroupRt>,
    pub selfsync: Arc<engine::SelfSync>,
    pub link: link::LinkState,
    pub link_events: Mutex<Option<Arc<dyn LinkSink>>>,
}

impl SyncRt {
    /// `selfsync` is the one registered as the `OwnDocSink` of `ctx`'s signal.
    pub fn new(ctx: Arc<NodeCtx>, chat: Arc<ChatRt>, group: Arc<GroupRt>, selfsync: Arc<engine::SelfSync>) -> Self {
        Self { ctx, chat, group, selfsync, link: Default::default(), link_events: Mutex::new(None) }
    }

    pub fn set_link_sink(&self, sink: Arc<dyn LinkSink>) {
        *self.link_events.lock() = Some(sink);
    }

    pub fn link_events(&self) -> Option<Arc<dyn LinkSink>> {
        self.link_events.lock().clone()
    }
}

impl Deref for SyncRt {
    type Target = NodeCtx;

    fn deref(&self) -> &NodeCtx {
        &self.ctx
    }
}
