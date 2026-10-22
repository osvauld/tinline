//! The device's one iroh endpoint and everything that rides on it: identity, contacts, and at
//! most one call at a time.
//!
//! Blocking methods are meant for the app's IO threads; long work (dialing, ringing, the call
//! itself) runs on this node's own tokio runtime and reports through `NodeEvents`.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use identity::Identity;
use iroh::endpoint::{Connection, QuicTransportConfig, presets};
use iroh::{Endpoint, EndpointAddr, PublicKey, RelayUrl, SecretKey, Watcher};
use parking_lot::{Mutex, ReentrantMutex};
use proto::{ContactTicket, Msg};
use sha2::{Digest, Sha512};
use tokio::sync::{OwnedMutexGuard, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use zeroize::Zeroize;

use crate::accounts::{self, AccountDirs};
use crate::store::{self, Backing, CallRecord, ContactDevice, Disk, History, OwnDevice, Profile, ProfileV2, State, Store, StoredContact};
use crate::vault::{self, Dek, Secrets};
use crate::wire::Ctrl;
use crate::Error;

mod api_call;
mod api_contacts;
mod api_identity;
mod hooks;
mod lifecycle;
mod rotation;
#[cfg(any(test, feature = "dev-tools"))]
mod testkit;
mod types;
mod unlock;
#[cfg(test)]
mod tests;

// The runtime (account session, endpoint, 1:1 call, contacts cluster, rotation) is the
// `node-runtime` crate; its items keep their `crate::node::` paths.
pub use node_runtime::*;
pub use self::types::*;

/// What the engines share through `Deref<Target = NodeCtx>`: the runtime context plus the
/// engines that still live in p2pcore. Hooks into the engines are `hooks.rs`.
pub(crate) struct Inner {
    pub(crate) ctx: Arc<NodeCtx>,
    pub(crate) chat: Mutex<Option<Arc<crate::chat::engine::ChatCore>>>,
    pub(crate) chat_events: Mutex<Option<Arc<dyn crate::chat::api::ChatEvents>>>,
    pub(crate) link_events: Mutex<Option<Arc<dyn crate::sync::LinkEvents>>>,
    pub(crate) selfsync: Arc<crate::sync::engine::SelfSync>,
    pub(crate) link: crate::sync::link::LinkState,
    /// Group calls (G6), `crate::gcall::net`.
    pub(crate) gcall: crate::gcall::net::GcallNet,
    /// G1: group chat runtime (second endpoint, sessions with hosts); see `group/engine.rs`.
    pub(crate) group: crate::group::engine::GroupRt,
    pub(crate) profile_events: Mutex<Option<Arc<dyn crate::profile::ProfileEvents>>>,
    pub(crate) profile_downloads: Mutex<HashSet<(u64, String, String)>>,
    pub(crate) profile_fetches: Arc<Semaphore>,
    /// The selected account's search index (`search.redb`), open while its chat store is.
    pub(crate) search: Mutex<Option<Arc<crate::search::index::SearchIndex>>>,
    /// For tests: the next `n` search hook writes are skipped (a crash between commit and index).
    pub(crate) search_skip: AtomicU32,
}

impl Inner {
    /// `commit_identity` with the gate held: the files, then the stores that open with them.
    pub(crate) fn commit_locked(self: &Arc<Self>) -> Result<(), Error> {
        if !self.commit_files()? {
            return Ok(());
        }
        self.chat_open();
        self.acc_open();
        // Anything that changed while the files were being written.
        self.persist().map_err(Error::from)
    }
}

impl std::ops::Deref for Inner {
    type Target = NodeCtx;

    fn deref(&self) -> &NodeCtx {
        &self.ctx
    }
}

/// Blocking methods (`start`, `stop`, `add_contact`, `my_ticket`) are for the app's own
/// threads; called from inside a `NodeEvents` callback they fail (or, for `stop`, go async)
/// instead of blocking the runtime that delivers the callback.
#[derive(uniffi::Object)]
pub struct Node {
    pub(crate) inner: Arc<Inner>,
    rt: Option<tokio::runtime::Runtime>,
}

impl Drop for Node {
    fn drop(&mut self) {
        let ep = self.inner.shared.lock().endpoint.take();
        if let Some(rt) = self.rt.take() {
            let can_block = !on_core_thread() && tokio::runtime::Handle::try_current().is_err();
            if can_block {
                let inner = self.inner.clone();
                rt.block_on(async move {
                    // Best effort: a clean close tells peers at once instead of at idle timeout.
                    if let Some(ep) = ep {
                        let _ = tokio::time::timeout(Duration::from_secs(1), ep.close()).await;
                    }
                    let _ = tokio::time::timeout(Duration::from_secs(3), inner.group_close()).await;
                    let _ = tokio::time::timeout(Duration::from_secs(3), inner.chat_close()).await;
                });
            }
            // GL: every store file goes now, whatever task still holds a clone (the tasks are
            // only dropped below, in the background), so a node made next on this directory
            // opens them at once.
            self.inner.group_close_now();
            self.inner.chat_close_now();
            self.inner.shared.lock().forget_session();
            // Never blocks, so it is safe even if the last reference drops on a core thread.
            rt.shutdown_background();
        }
    }
}

impl Node {
    /// Runs `fut` on the core runtime and waits for it. Waiting on a plain channel works from
    /// any app thread, including another runtime's (iced's tokio, `spawn_blocking`); only the
    /// core's own threads are refused, since blocking one could starve the future itself.
    pub(crate) fn block_on<T: Send + 'static>(
        &self,
        fut: impl std::future::Future<Output = T> + Send + 'static,
    ) -> Result<T, Error> {
        if on_core_thread() {
            return Err(Error::Protocol("blocking call made from a NodeEvents callback".into()));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.inner.handle.spawn(USER_CALL.scope((), async move {
            let _ = tx.send(fut.await);
        }));
        rx.recv().map_err(|_| Error::Protocol("core runtime stopped".into()))
    }

    /// Takes the lifecycle lock for the caller's whole operation; refuses on core threads.
    fn lifecycle(&self) -> Result<OwnedMutexGuard<()>, Error> {
        let l = self.inner.lifecycle.clone();
        self.block_on(async move { l.lock_owned().await })
    }

    /// Runs a lifecycle step and waits for it; from a callback it can't wait, so it finishes in
    /// the background.
    fn run_lifecycle(&self, fut: impl std::future::Future<Output = ()> + Send + 'static) {
        if on_core_thread() {
            self.inner.handle.spawn(fut);
        } else {
            let _ = self.block_on(fut);
        }
    }

    /// Runs a slow, blocking computation (the KDF) on the runtime's blocking pool and waits for
    /// it, so it never runs while `shared` is held and never on a callback thread.
    pub(crate) fn kdf<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Result<T, Error> {
        self.block_on(async move {
            tokio::task::spawn_blocking(f).await.map_err(|_| Error::Protocol("kdf task failed".into()))
        })?
    }
}
