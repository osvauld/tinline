//! p2pcore's side of the node runtime's seam: `NodeHooks` on `Inner`, calling the engines.
//! Installed right after `Arc::new(Inner { .. })` (`api_identity.rs`).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use iroh::endpoint::{Accepting, Connection};
use node_runtime::wire::Ctrl;
use node_runtime::hooks::NodeHooks;
use node_runtime::{BoxFuture, Me, RotationPolicy};
use tokio::sync::OwnedSemaphorePermit;

use super::Inner;
use crate::Error;

impl NodeHooks for Inner {
    fn group_unpark(self: Arc<Self>) {
        // GL: the group runtime may open its store from background tasks again.
        let _slot = self.group.core.lock();
        self.group.parked.store(false, Ordering::SeqCst);
    }

    fn chat_started(self: Arc<Self>) {
        chat_runtime::ChatRt::chat_started(&self.chat);
    }

    fn selfsync_started(self: Arc<Self>) {
        Inner::selfsync_started(&self);
    }

    fn group_started(self: Arc<Self>) {
        group_runtime::GroupRt::group_started(&self.group);
    }

    fn group_invites_started(self: Arc<Self>) {
        group_runtime::GroupRt::group_invites_started(&self.group);
    }

    fn chat_sessions_close(self: Arc<Self>) {
        chat_runtime::ChatRt::chat_sessions_close(&self.chat);
    }

    fn gcall_shutdown(self: Arc<Self>) -> BoxFuture<()> {
        Box::pin(async move { self.group.gcall_shutdown().await })
    }

    fn group_close(self: Arc<Self>) -> BoxFuture<()> {
        Box::pin(async move { self.group.group_close().await })
    }

    fn chat_close(self: Arc<Self>) -> BoxFuture<()> {
        Box::pin(async move { self.chat.chat_close().await })
    }

    fn selfsync_sessions_close(self: Arc<Self>) {
        for slot in self.selfsync.sessions.lock().values() {
            slot.close();
        }
    }

    fn incoming_self(self: Arc<Self>, accepting: Accepting) -> BoxFuture<Result<(), node_runtime::Error>> {
        Box::pin(async move { self.handle_self_incoming(accepting).await.map_err(Into::into) })
    }

    fn incoming_link(self: Arc<Self>, accepting: Accepting) -> BoxFuture<Result<(), node_runtime::Error>> {
        Box::pin(async move { self.handle_link_incoming(accepting).await.map_err(Into::into) })
    }

    fn incoming_chat(self: Arc<Self>, accepting: Accepting, chat: bool, permit: OwnedSemaphorePermit) -> BoxFuture<Result<(), node_runtime::Error>> {
        Box::pin(async move { self.chat.clone().handle_chat_incoming(accepting, chat, permit).await.map_err(Into::into) })
    }

    fn chat_conn(self: Arc<Self>, conn: Connection, chat: bool, permit: OwnedSemaphorePermit) -> BoxFuture<Result<(), node_runtime::Error>> {
        Box::pin(async move { self.chat.clone().handle_chat_conn(conn, chat, permit).await.map_err(Into::into) })
    }

    fn group_letter(self: Arc<Self>, ctrl: Ctrl, letter: proto::Msg, remote: [u8; 32], me: Arc<Me>) -> BoxFuture<Result<(), node_runtime::Error>> {
        Box::pin(async move { self.group.group_letter_incoming(ctrl, letter, remote, &me).await })
    }

    fn in_group_call(&self) -> bool {
        self.group.gcall.in_call()
    }

    fn chat_call_activity(self: Arc<Self>, did: &str) {
        chat_runtime::ChatRt::chat_call_activity(&self.chat, did);
    }

    fn chat_close_sessions(self: Arc<Self>, did: &str) {
        chat_runtime::ChatRt::chat_close_sessions(&self.chat, did);
    }

    fn chat_kick(self: Arc<Self>, did: String) {
        chat_runtime::ChatRt::chat_kick(&self.chat, did);
    }

    fn chat_kick_pending(self: Arc<Self>) {
        chat_runtime::ChatRt::chat_kick_pending(&self.chat);
    }

    fn group_rotation_policy(self: Arc<Self>) -> Result<RotationPolicy, node_runtime::Error> {
        let core = self.group.group_core()?;
        self.group.group_rotation_policy(&core)
    }

    fn profile_for_new_contact(self: Arc<Self>, did: &str) {
        chat_runtime::ChatRt::profile_for_new_contact(&self.chat, did);
    }
}
