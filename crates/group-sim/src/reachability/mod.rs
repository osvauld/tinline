//! Group consumers of a MOCK verified-succession adapter. Rotation task owns the actual
//! signature/path verifier. These fixtures do not prove succession cryptography or expiry policy.
//!
//! R1b: content here uses the LEGACY device-key-signed `Envelope` fixture (`tests.rs`; superseded
//! by U4 revised + U16 DID-signed posts with an app-supplied writer ID, see `host_only.rs`). The
//! envelope is only a carrier; the properties tested here do not depend on how a post is signed.
use super::*;

mod scheduler;

mod adapter;

use adapter::{Nomination, BindingEvidence, Audience, VerifiedSuccession, MockRotation, HostDirectory, RouteFailure, Network, nomination, evidence, successor, NoVerifiedPath};

mod tests;
