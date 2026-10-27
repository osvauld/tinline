//! Several accounts reachable on this computer (`docs/multi-online.md`): the desktop runs every
//! account with a remembered key in the background (`BackgroundMode::KeepAll`). Here: the core's
//! `AccountEvents` bridge and the labels the ring, notifications and the switcher show for an
//! account that is not the open one.

use std::path::PathBuf;

use p2pcore::AccountRef;
use tokio::sync::mpsc;

use crate::Ev;

/// Bridges `AccountEvents` into the same channel as `NodeEvents`; answers `account_key` from the
/// OS keyring.
pub struct AccountBridge {
    pub tx: mpsc::UnboundedSender<Ev>,
    pub data: PathBuf,
}

impl p2pcore::AccountEvents for AccountBridge {
    /// Asked on the thread of `start` (always a blocking worker on desktop, never the UI thread).
    fn account_key(&self, did: String) -> Option<Vec<u8>> {
        crate::keystore::remembered_key(crate::keystore::system(), &self.data, &did)
    }
    fn on_background_message(&self, account: AccountRef, from_did: String, from_name: String) {
        crate::tlog!("BACKGROUND_MESSAGE {} from {from_did}", account.did);
        let _ = self.tx.send(Ev::Background {
            account,
            from_did,
            from_name,
        });
    }
    fn on_accounts_changed(&self) {
        let _ = self.tx.send(Ev::Accounts);
    }
}

fn or_someone(name: &str) -> &str {
    if name.trim().is_empty() {
        crate::i18n::tr_str("common_someone")
    } else {
        name
    }
}

/// Who rings or wrote, and to which account: "Asha → Work"; just "Asha" for the open account.
pub fn ring_name(peer: &str, account: Option<&AccountRef>) -> String {
    match account {
        Some(a) => crate::i18n::tr_args(
            "accounts_ring_name",
            &[("name", or_someone(peer)), ("account", &a.name)],
        ),
        None => peer.to_string(),
    }
}

/// The ring's notification line: "Asha is calling you on Work" for another account.
pub fn calling_line(peer: &str, account: Option<&AccountRef>) -> String {
    let empty = peer.trim().is_empty();
    match account {
        Some(a) if empty => crate::i18n::tr_args(
            "accounts_someone_is_calling_you_on",
            &[("account", &a.name)],
        ),
        Some(a) => crate::i18n::tr_args(
            "accounts_is_calling_you_on",
            &[("name", peer), ("account", &a.name)],
        ),
        None if empty => crate::i18n::tr("common_someone_is_calling"),
        None => crate::i18n::tr_args("common_is_calling", &[("name", peer)]),
    }
}

/// The line under another account in the switcher.
pub fn switch_line(unread: u32) -> String {
    if unread == 0 {
        crate::i18n::tr("accounts_tap_to_switch")
    } else {
        crate::i18n::tr_count("accounts_new_tap_to_switch", unread.into(), &[])
    }
}

#[cfg(test)]
#[path = "accounts_bg_tests.rs"]
mod tests;
