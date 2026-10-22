//! The own-device sync engine is `sync_runtime::engine`; the tests here run whole `Node`s, so
//! they stay with p2pcore at their old module paths.

pub(crate) use sync_runtime::engine::*;
#[cfg(test)]
mod group_contents;
#[cfg(test)]
mod group_history;
#[cfg(test)]
mod group_receipts;
