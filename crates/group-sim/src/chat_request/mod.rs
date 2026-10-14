//! W2-Q: 1:1 chat request routed through the group host (U3). Blocking is visible (U5
//! revised); what a blocker returns to a chat request is open (section 8b Q4) and modelled as
//! the `BlockReply` candidates. R1b removed the tests that asserted the rejected silent option.
//!
//! Status: proposal — not approved. Simulation only; no production API, transport or UI.
//!
//! Model: every member publishes a DID-signed X25519 *request key* in the group (the host
//! keeps a directory of the latest note per DID). A requester seals a chat request to the
//! target's request key and hands it to the host, which only sees routing metadata
//! (`Header`). If the target accepts, it seals an answer back to the requester's request
//! key. The private endpoints travel only inside the sealed, DID-signed payloads; after
//! acceptance the two chat directly and the host is not involved.
//!
//! The "host" here is the mailbox role of the group's relay replica (the `Replica` used as a
//! host in `tests.rs`/`reachability.rs`); it is modelled as a separate struct because chat
//! requests are not group envelopes and must never enter the shared Loro document.
//!
//! !!! SIMULATION CONSTRUCTION, NOT AN APPROVED PRODUCTION SCHEME !!!
//! Sealing = ephemeral X25519 -> blake3::derive_key (domain-separated context, input =
//! shared || ephemeral_pub || recipient_pub) -> AES-256-GCM with the serialized routing header
//! as AAD and a fixed zero nonce (safe only because every seal uses a fresh ephemeral key, so
//! the derived key is single-use). The sender's DID signature is *inside* the ciphertext, and
//! the signed payload names group, sender, recipient and key sequence, so a re-encrypting
//! insider cannot re-address it. Plaintexts are padded to a fixed bucket so accept/decline
//! sizes match. Production must pick a reviewed construction (e.g. HPKE RFC 9180 base or
//! auth mode, or a Noise pattern) — OPEN QUESTION, not decided here.
//!
//! Ephemeral keys, request IDs and request-key secrets come from a deterministic blake3
//! "RNG" keyed by fixture seeds. Never production randomness.
use crate::legacy::*;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use x25519_dalek::{PublicKey, StaticSecret};

mod crypto;
mod harness;
mod members_host;

use crypto::{
    ANSWER_TIMEOUT, Answer, BlockReply, DeclinePolicy, Delivery, DirectBody, DirectMsg, DropReason,
    GROUP, Header, Incoming, Inner, KeySlot, MemberState, Observation, Outgoing, PAD, Policy,
    RateLimit, Relay, RequestKeyClaim, RetiredKeyPolicy, SignedInner, SignedRequestKey, Wire,
    WireKind, contains, did_sign, did_verify, hex, open, policy, seal, sim_random,
};
use harness::{assert_rejected, flip, pump, send};
use members_host::{Host, HostRefusal, Member, route_direct, world};

mod tests;
