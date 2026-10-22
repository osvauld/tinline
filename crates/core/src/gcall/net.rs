//! Group calls on iroh: the service lives in the `gcall-net` crate (plain Rust, no uniffi) and
//! is re-exported here so `p2pcore::gcall::net::*` keeps working; the `Node` entry points and the
//! node's [`GcallHost`] implementation are in `node`.

pub use gcall_net::{
    CLOSE_ANSWERED_ELSEWHERE, CLOSE_BYE, CLOSE_DROPPED, CLOSE_ENDED, CLOSE_REFUSED, CLOSE_REMOVED, CallDirectory, ClientStatus,
    FOLLOW_WINDOW, GcallConfig, GcallEvent, GcallEvents, GcallHost, GcallNet, GcallStatus, HostingStatus, StaticDirectory, close_code,
};

use super::*;

mod node;
