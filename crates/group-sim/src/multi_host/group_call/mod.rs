//! G6: group calls relayed through one call host (U21). Simulation tests GC-S01..GC-S17 from
//! `docs/design/group-call-spec.md` §7.
//!
//! Status: tests first. Simulation only; no production API, wire format, transport, audio or UI.
//! This file is a child module of `multi_host.rs`, so it reuses that module's `World` without
//! widening visibility. The reused parts are the signed genesis and grants, DID-signed group
//! endpoint notes, host records and the chat-host directory (U7), admin-signed ops, U9 flag
//! removal, `rotate_host`/`rotate_member` (U12), U5-revised block notices and the rogue-host
//! fixture.
//!
//! Model (test-only):
//! - A call-host permit is the multi-host permit leaf with `Cap::CallHost` (§2.1), issued by the
//!   group creator and bound to one device slot. A call-host record is `HostRecord { note,
//!   permit }`. Members accept it like a chat-host record, but they check `CallHost`.
//! - Signed call control (§3.1): `CallStart`, `Join`, `Roster`, `Leave` and `End` are signed by
//!   the DID with real Ed25519. They use a simulation domain that stands in for
//!   `tinline/gcall/v1\0`. The `Heartbeat` is unsigned, because it travels on the authenticated
//!   stream.
//! - The host's forwarder is the pure function `select` over RFC 6464 levels that the senders
//!   report (§1.3). It has a moving average, hysteresis and the §6 dominance cap.
//! - Ranking (§2.3) and the phone degrade policy (§2.4) are pure functions as well.
//! - Time is fake milliseconds. Whether a dial succeeds is decided when the dial is made. The
//!   5 s timeout per backup (§5) is modelled as elapsed time on the redial log, not as a
//!   running timer.
//! - The `CallStart` is relayed through the chat hosts as a group `Ephemeral` message. Here it
//!   is a frame from chat host `H2` to every device. `hold_relay` delays that relay so that
//!   splits and glare can be modelled.
//!
//! Spec gaps this model has to fill are listed in `ASSUMPTIONS` below. Each one is marked
//! `ASSUMPTION(Gx)` at the code that uses it.
use super::*;
use std::collections::VecDeque;

mod protocol;
mod sim_state;
mod sim_hosts;
mod sim_media;
mod sim_driver;

use protocol::{CAP_DESKTOP, CAP_PHONE_METERED, K_PHONE, GROUP_BITRATE_BPS, REDIAL_WINDOW_MS, ROTATION_DEFER_CAP_MS, HEARTBEAT_MS, HEARTBEAT_MISSES, MAX_UP_PPS, STATUS_TTL_MS, UP_BURST, MAX_DATAGRAM, MAX_CONTROL, MAX_CONTROL_PER_S, JOIN_MIN_GAP_MS, ANSWER_TIMEOUT_MS, MEMBER_DROP_MS, MAX_BACKUPS, FRAME_MS, SPEECH_DBOV, AVG_FRAMES, HYST_FRAMES, DOM_SHARE_PCT, ROTATION_DELAY_MS, DRED_BYTES, AUDIO_HDR, GROUP_HDR, QUIC_BYTES, UDP_IP4, STATUS_DOMAIN, UP, DOWN, LEVEL_ONLY, GATE_FRAMES, LEVEL_ONLY_FRAMES, HostRef, EndReason, RosterEntry, Kind, CallMsg, Ctl, Announce, JoinReq, Active, Heartbeat, Class, Thermal, Power, Cond, PhoneAction, phone_policy, StatusBody, Status, TIER_UNLISTED, tier, tie_hash, call_hash, rank, Fwd, select};
use sim_state::{copies, mix, audio, CR, Part, HostCall, Dialing, Joined, CallDev, Redial, CALL_HOSTS, Sim};
use sim_driver::{base, roster_of, assert_unchanged};

mod tests;
