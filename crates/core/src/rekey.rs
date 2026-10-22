//! The keys of the sealed stores: `node_runtime::rekey`, kept under the old path.

pub use node_runtime::rekey::*;

#[cfg(test)]
#[path = "rekey_tests.rs"]
mod tests;
