//! The own-device sync engine is `sync_runtime::engine`; the tests here run whole `Node`s, so
//! they stay with p2pcore at their old module paths.

pub(crate) use sync_runtime::engine::*;
#[cfg(test)]
#[allow(unused_imports)]
use {
    std::collections::{HashMap, HashSet},
    std::sync::Arc,
    std::time::{Duration, Instant},
    iroh::endpoint::{Accepting, Connection},
    loro::VersionVector,
    sync_runtime::accdoc::{AccDoc, DOC_NAME},
    sync_runtime::callsdoc::{CALLS_DOC, CallsDoc},
    sync_runtime::doc::Doc,
    sync_runtime::frames::{FrameReader, FrameWriter, SelfMsg},
    sync_runtime::profilecache::PROFILE_CACHE_DOC,
    sync_runtime::profiledoc::PROFILE_DOC,
    sync_runtime::SyncRt,
    sync_runtime::Error,
    chat_runtime::chat::own::{ACK_MARK, GROUPS_MARK, MAX_OWN_DOCS, READ_MARK, parse_doc},
    group_runtime::own::contents as group_documents,
    node_runtime::{Me, addr_for, now, relay_of, CALLS_KEY, DOC_KEY},
    node_runtime::store::{Backing, OwnDevice},
};

#[cfg(test)]
mod group_contents;
#[cfg(test)]
mod group_history;
#[cfg(test)]
mod group_receipts;
