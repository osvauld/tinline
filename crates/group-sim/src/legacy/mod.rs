//! Legacy device-signed `Envelope` carrier: `Author`, insert-only Loro `Replica`, pending queue
//! and causal repair. Superseded as a content model (see the note in `envelope.rs`), kept because
//! the persistence, policy-candidate, scheduler, reachability and block-in-group models use it
//! as a carrier. Every model module imports this module's items with `use crate::legacy::*;`.
pub(crate) use crate::authority::{
    Capability, Genesis, Grant, PostAuthorization, SignedGroupAuthority,
};
pub(crate) use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
pub(crate) use identity::Identity;
pub(crate) use loro::{ExportMode, LoroDoc};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use std::collections::{BTreeMap, BTreeSet};

mod envelope;
mod fixtures;
mod replica;

pub(crate) use envelope::*;
pub(crate) use fixtures::*;
pub(crate) use replica::*;

mod tests;
