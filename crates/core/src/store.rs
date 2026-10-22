//! What survives a restart, in one account's directory. The sealed stores and the call log are
//! `node_runtime::store`; this module keeps UniFFI's declaration of `CallRecord` and the old path.

pub use node_runtime::store::*;

// UniFFI's declaration of `CallRecord` (its model is `account_model::store`).
mod models;
