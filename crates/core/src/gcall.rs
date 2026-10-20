//! Group calls (G6): the sans-IO engines live in the `gcall-core` crate and are re-exported
//! here so `p2pcore::gcall::*` keeps working; [`net`] runs them on iroh connections.

pub use gcall_core::*;

pub mod net;
