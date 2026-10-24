//! The own-device docs (`tinline/self/3`): the account doc, the call log, the account profile and
//! the contact profile cache, with their merge rules. p2pcore owns linking and the sessions.

pub mod accdoc;
#[cfg(test)]
mod accdoc_tests;
pub mod callsdoc;
pub mod profilecache;
pub mod profiledoc;
#[cfg(test)]
mod profiledoc_tests;
/// The own-device doc wrapper lives in the `crdt` crate; `io` maps any error into ours.
pub mod doc {
    pub use crdt::doc::{Doc, entries, field, text};

    pub fn io<E: std::fmt::Display>(e: E) -> crate::Error {
        crate::Error::Io(e.to_string())
    }
}
