//! Five-node network scheduler; addresses/succession verification remain mock adapters.
//! In-memory replicas, bounded queues, original signed messages. No disk or real sockets.
//!
//! R1b (U7): nodes 0-2 are members and nodes 3-4 are hosts (multi-host is decided). A member
//! holds routes to the hosts only, never to another member; hosts hold routes to every node.
//! Every send, reconcile and address push between two members fails with `NoAddress` by
//! construction, so all member traffic goes through a host. Before R1b every node held a route
//! to every other node and members synced with each other. Content still uses the legacy
//! device-signed `Envelope` fixture (`tests.rs`) as a carrier.
use super::*;

mod model;

use model::{
    BINDING_LIFETIME, FIRST_HOST, Fleet, NODES, Payload, QUEUE_LIMIT, SendFailure, is_host,
};

mod tests;
