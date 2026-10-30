//! Everything the node runtime needs from the engines above it, in one object-safe trait.
//!
//! p2pcore implements [`NodeHooks`] on its `Inner` and installs a `Weak` to it right after
//! building the `Arc` ([`NodeCtx::install_hooks`](crate::NodeCtx::install_hooks)). Calls are
//! direct, not events: the start sequence awaits the rotation inline and `stop` must flush the
//! group call and close the chat before the endpoint goes, so a channel would change the timing.
//! With no engine (the node is dropping) a caller does what it would with nothing to call: it
//! returns or does nothing.
//!
//! **No `NodeCtx` data lock may be held across a hook call** (crate docs, "Lock order"), with two
//! documented exceptions: the tokio mutexes `lifecycle` and `closing`, which the start, stop and
//! rotation sequences hold on purpose, and [`NodeHooks::in_group_call`], which is called with
//! `live` held (lock order `live -> group-call session`, as `with_no_call`). Implementations must
//! never take `lifecycle`, `closing` or `live`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use iroh::endpoint::{Accepting, Connection};
use tokio::sync::OwnedSemaphorePermit;

use crate::wire::Ctrl;
use crate::{Error, Me};

/// A boxed future, for the few async hooks (the trait must stay object-safe).
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub trait NodeHooks: Send + Sync + 'static {
    // --- start (in this order, interleaved with the contacts cluster's own starts)
    /// The group engine may open its store from background tasks again.
    fn group_unpark(self: Arc<Self>);
    fn chat_started(self: Arc<Self>);
    fn selfsync_started(self: Arc<Self>);
    fn group_started(self: Arc<Self>);
    fn group_invites_started(self: Arc<Self>);

    // --- stop, lock, account switch, endpoint close
    /// Closes every chat session (the endpoint is about to go).
    fn chat_sessions_close(self: Arc<Self>);
    /// Leaves (or, hosting, ends) the group call before the endpoint closes.
    fn gcall_shutdown(self: Arc<Self>) -> BoxFuture<()>;
    /// The group endpoint goes down with the private one.
    fn group_close(self: Arc<Self>) -> BoxFuture<()>;
    /// Chat engine: sessions and the store.
    fn chat_close(self: Arc<Self>) -> BoxFuture<()>;
    /// Own-device sync sessions end (the private endpoint rotated).
    fn selfsync_sessions_close(self: Arc<Self>);

    // --- incoming connections, by ALPN
    /// `proto::SELF_ALPN`: an own device.
    fn incoming_self(self: Arc<Self>, accepting: Accepting) -> BoxFuture<Result<(), Error>>;
    /// `proto::link::LINK_ALPN`: linking a new device.
    fn incoming_link(self: Arc<Self>, accepting: Accepting) -> BoxFuture<Result<(), Error>>;
    /// The chat ALPN (`chat`) or the blob ALPN.
    fn incoming_chat(self: Arc<Self>, accepting: Accepting, chat: bool, permit: OwnedSemaphorePermit) -> BoxFuture<Result<(), Error>>;
    /// An accepted connection on the chat ALPN (from the previous endpoint while it overlaps).
    fn chat_conn(self: Arc<Self>, conn: Connection, chat: bool, permit: OwnedSemaphorePermit) -> BoxFuture<Result<(), Error>>;
    /// A group letter (invitation, welcome) arrived on the contact ALPN.
    fn group_letter(self: Arc<Self>, ctrl: Ctrl, letter: proto::Msg, remote: [u8; 32], me: Arc<Me>) -> BoxFuture<Result<(), Error>>;

    // --- questions and nudges
    /// A group call is up (the user is in one). Called with `live` held.
    fn in_group_call(&self) -> bool;
    /// A 1:1 call with `did` started or ended: its chat sessions are not idle.
    fn chat_call_activity(self: Arc<Self>, did: &str);
    /// Closes the chat sessions with `did` (blocked).
    fn chat_close_sessions(self: Arc<Self>, did: &str);
    /// Dials `did` for chat now (unblocked).
    fn chat_kick(self: Arc<Self>, did: String);
    /// Retries every contact with something pending (network change, rotation).
    fn chat_kick_pending(self: Arc<Self>);
    /// The rotation policy in force; `Err` while the group store is not open.
    fn group_rotation_policy(self: Arc<Self>) -> Result<crate::RotationPolicy, Error>;
    /// A contact was added or synced in: fetch its profile picture.
    fn profile_for_new_contact(self: Arc<Self>, did: &str);
    /// An own device woke us: pictures still pending are tried again now, not after their
    /// backoff.
    fn profile_retry_now(self: Arc<Self>) {}
}
