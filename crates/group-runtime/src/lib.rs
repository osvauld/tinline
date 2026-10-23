//! The group runtime (G1): the group endpoint (U2, `tinline/group/5`), host and member sessions,
//! replication and catch-up, removal enforcement, direct invitations and invite codes (G3), chat
//! requests (U3), presence and image posts (public group), member receipts and group calls
//! (G6, over `gcall_net`). No UniFFI. See `docs/group-core.md`.
//!
//! [`GroupRt`] holds this runtime's state and derefs to the node's [`NodeCtx`](node_runtime::NodeCtx),
//! so the engine reads the account session, the endpoint and the contacts cluster directly.
//! p2pcore builds one per node, declares the records in [`api`], [`calls`], [`invitations`],
//! [`requests`], [`member_receipts`] and [`calllog`] to UniFFI and forwards the sinks
//! ([`GroupSink`], [`calls::GroupCallSink`], [`invitations::GroupInvitationSink`],
//! [`requests::GroupRequestSink`]) to the app's callbacks.
//!
//! The model (records, docs, derived state, day shards, the group vault, sealed mail, rotation
//! policy, code links, stored invitation/request rows) is the `group-core` crate, re-exported
//! here. Locks: `NodeCtx`'s order applies (`node_runtime` crate docs); `core` is held while the
//! store opens. Everything `pub` here beyond `GroupRt`'s constructor and the records is for
//! p2pcore's facade and the sync engine, not a stable API.

pub use group_core::{doc, mail, record, rotation, shard, state};
pub use group_core::{short, valid_gid};
pub use node_runtime::Error;

mod access;
mod checked;
pub mod api;
pub mod attestation;
pub mod calldir;
pub mod calllog;
pub mod calls;
pub mod code;
pub mod engine;
pub mod images;
pub mod invitations;
pub mod member_receipts;
pub mod own;
mod relay;
pub mod requests;
mod search;
pub mod store;
pub mod wire;

pub use api::GroupSink;
pub use engine::GroupRt;
