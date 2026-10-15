//! IO wiring for group calls (GA-P4): runs [`HostEngine`] and [`ClientEngine`] on real iroh
//! connections on `tinline/gcall/1` (§1.1).
//!
//! **Unstable.** This is the core-side API that group chat and the UI call; it is plain Rust
//! (no uniffi) and may change. Membership, permits, group endpoints and host addresses all
//! come through [`CallDirectory`]; group chat supplies it (`group::calldir::GroupCallDir`).
//!
//! Shape:
//!
//! - One [`Session`] per node at a time: the call this device is in and/or hosts. It holds both
//!   engines behind one lock. Engine actions are executed synchronously under that lock
//!   (control frames go to a per-connection writer task, datagrams go out at once), so the
//!   host's own user is routed through [`ConnId::LOCAL`] without a network hop: a `LOCAL`
//!   control frame or datagram from one engine is fed straight into the other.
//! - Each remote connection has one reader task (control stream + datagrams) and one writer
//!   task (control stream, then the close).
//! - A 20 ms ticker per session drives `HostEngine::tick` / `ClientEngine::tick` and a
//!   level-only keepalive. Audio itself is driven by the app's audio threads, exactly like
//!   1:1 calls: [`Node::push_mic`](crate::Node::push_mic) / `pull_speaker` fall through to the
//!   group call when no 1:1 call is active (or use [`crate::Node::gcall_push_mic`] directly).
//! - Close codes ([`close_code`]): a client that sees [`CLOSE_ANSWERED_ELSEWHERE`] ends with
//!   "answered elsewhere"; any other close while joined starts the §5 redial.
//! - Takeover (§5): a node whose `host_ref` is in the `backups` of a call it knows (the call
//!   its own client is in, or one fed through [`crate::Node::gcall_observe_start`]) builds a
//!   `HostEngine::takeover` on the first `Join{host_gen + 1}`, from the network or from its
//!   own redialing client. If that first Join is refused the engine is dropped again.
//! - Glare and split (§3.1, §5): every CallStart this node learns of goes on a per-group
//!   [`CallBoard`]; when the call hosted here lost, it ends with `End{superseded, next_host:
//!   winner}` and its members follow the winner (at once if they know its CallStart, else when
//!   it arrives, within [`FOLLOW_WINDOW`]).
//! - One call at a time: while this device's user is in a group call a 1:1 call is refused
//!   (incoming: Busy), and a group call cannot be started or joined during a 1:1 call.
//!   `Node::stop` and `Node::lock` leave the call and, when hosting, send `End{host_leaving,
//!   next_host}` before the endpoint closes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use identity::Identity;
use iroh::endpoint::{Accepting, Connection, ConnectionError, RecvStream, SendStream};
use iroh::EndpointAddr;
use parking_lot::Mutex;
use proto::gcall::{self as w, CallStart, GCALL_ALPN, GroupCallMsg};
use proto::gcall::{SignedEnd, SignedNote, SignedStart};
use tokio::sync::mpsc;

use super::*;
use crate::Error;
use crate::node::Inner;

mod config;
mod link;
mod node;
mod service;
mod session;

pub use config::*;
pub(crate) use service::Service;
use service::*;
use session::*;
use link::*;
