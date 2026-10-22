//! The keys of the sealed stores are `node_runtime::rekey`; their tests stay here.

#[cfg(test)]
use node_runtime::rekey::*;

#[cfg(test)]
#[path = "rekey_tests.rs"]
mod tests;
