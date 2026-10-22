//! The receipt digest of a DM message version is `chat_core::doc::dm_version` (it reads the chat
//! record); the issuance ledger it feeds is `receipts_core::store::messages`.
#[cfg(test)]
use crate::chat::doc::dm_version;

#[cfg(test)]
mod tests;
