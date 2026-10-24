//! Group calls (GA-I1). The runtime is `group_runtime::calls`; this declares its records to
//! UniFFI (`types.rs`) and holds the `Node` calls (`node.rs`).

pub use group_runtime::calls::*;

mod node;
mod types;
pub use types::*;
