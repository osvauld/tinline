//! Core integration tests, linked once.
//!
//! Filter by module: `--test it -- chat_shards::`.
//! Calls only: `--test it -- calls:: --skip video_calls::` (substring filters overlap).

#[path = "../support/mod.rs"]
mod support;

mod accounts;
mod blocking_acceptance;
mod call_tokens_acceptance;
mod calls;
mod chat_shards;
mod contact_requests;
mod device_list_characterization;
mod device_list_push;
mod devices;
mod endpoint_close;
mod fanout;
mod gcall;
mod golden_core;
mod grant_characterization;
mod group_call;
mod group_core;
mod group_files;
mod group_g2;
mod group_g3;
mod group_shards;
mod group_voice;
mod idle_close;
mod lifecycle;
mod link;
mod link_expiry;
mod message_receipts;
mod multi_online;
mod multi_online_edges;
mod multi_online_review;
mod old_writer_delivery;
mod own_wake;
mod private_rotation;
mod profile_sync;
mod public_group;
mod readd;
mod restore;
mod restore_characterization;
mod restore_ticket;
mod search;
mod tickets;
mod vault;
mod video_calls;
mod writer_id;
