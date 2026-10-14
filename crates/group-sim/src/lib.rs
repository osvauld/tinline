//! Test-only group simulation. Not a production protocol or authorization API.
//!
//! Signed genesis/delegation fixtures, insert-only messages, proofs and real Loro merging
//! (`legacy`, `authority`), plus one directory module per decision model: host-only and
//! multi-host groups, removal by admin flags, announced host rotation, 1:1 chat requests,
//! personal blocking, causal policy candidates, durable/volatile replicas with crash injection,
//! succession-based reachability, the star scheduler and call hosts. See `README.md`.
#[cfg(test)]
mod authority;
#[cfg(test)]
mod common;
#[cfg(test)]
mod legacy;

#[cfg(test)]
mod block_in_group;
#[cfg(test)]
mod chat_request;
#[cfg(test)]
mod flags;
#[cfg(test)]
mod host_only;
#[cfg(test)]
mod host_relay;
#[cfg(test)]
mod multi_host;
#[cfg(test)]
mod persistence;
#[cfg(test)]
mod policy_candidates;
#[cfg(test)]
mod reachability;
#[cfg(test)]
mod removal_rotation;
