//! GC-S cases from `crates/group-sim/src/group_call.rs`, run against the real engines, plus
//! G6-D2. A small in-memory world routes engine actions between devices.

mod failover;
mod forwarding;
mod glare_split;
mod joins;
mod limits;
mod mix_alloc;
mod stall;
mod world;
