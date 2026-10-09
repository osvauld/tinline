//! Test-only group simulation. Not a production protocol or authorization API.
//! Signed genesis/delegation fixtures, insert-only messages, proofs and real Loro merging.
//! Rotation, revocation, edits and durable storage are not modeled yet.
#[cfg(test)]
mod authority;
#[cfg(test)]
mod tests;
