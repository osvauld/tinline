//! G-C persistence/crash harness for group-chat-tests.md §4 (S01–S07). Test-only.
//!
//! A host replica has an explicit `Durable` record (the "disk") and `Volatile` state (the
//! in-memory `Replica`). A durable write is ONE ideal atomic struct assignment (`Host::write`).
//! A crash drops every volatile field; recovery (`Host::open`) rebuilds the replica only from
//! durable originals and re-verifies every proof. Faults are injected before/after, or as a
//! returned error at, each pipeline step: Validate, ScratchImport, Commit, StoredAck, FanOut.
//!
//! NOT modelled: fsync, power loss, torn/partial writes, redb transactions, vault sealing/AEAD,
//! disk-full, Android process death, real sockets. Process-crash coverage here is not evidence of
//! filesystem or power-loss durability (docs/group-persistence-contract.md).
//!
//! R1b: content here uses the LEGACY device-key-signed `Envelope` fixture (`tests.rs`; superseded
//! by U4 revised + U16 DID-signed posts with an app-supplied writer ID, see `host_only.rs`). The
//! envelope is only a carrier; the properties tested here do not depend on how a post is signed.
use super::*;
use loro::{LoroValue, VersionVector};

mod crash_model;
mod runner;

use crash_model::{Step, Fault, all_faults, AckOrder, PendingPolicy, OpenPolicy, encode, decode, Durable, Canonical, canonical, View, view, Net, Ingest, OpenReport, Availability, Host, Sender, deliver, reconcile, fresh, Fx, fixture, populated, control};
use runner::{run, expect_injected, restart_if_down, recover, assert_converged};

mod tests;
