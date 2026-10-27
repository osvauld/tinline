//! `Node` API: several accounts reachable on one device (`docs/multi-online.md`, UniFFI-exported).
//! The slots themselves are `slots.rs`.

use super::*;

#[uniffi::export]
impl Node {
    /// Which other accounts run besides the open one. Set it before `start`; `OpenOnly` (the
    /// default) closes any that run now.
    pub fn set_background_mode(&self, mode: BackgroundMode) {
        self.hub.set_mode(mode);
        if mode == BackgroundMode::OpenOnly {
            let background = self.hub.take_woken();
            if !background.is_empty() {
                self.close_slots(background);
                self.hub.accounts_changed();
            }
        }
    }

    pub fn background_mode(&self) -> BackgroundMode {
        self.hub.mode()
    }

    /// Registers the platform's side of background accounts: the remembered keys
    /// (`account_key`) and the notices of a background account. Without it no other account is
    /// ever opened in the background.
    pub fn set_account_events(&self, events: Arc<dyn AccountEvents>) {
        self.hub.set_account_events(events);
    }
}

impl Node {
    /// Closes slots taken out of the hub, waiting for it when it can (as `lock` does).
    pub(crate) fn close_slots(&self, slots: impl IntoIterator<Item = Arc<Inner>>) {
        // Each counts as closing from here (`close_slot`), before the first one is polled.
        let closes: Vec<_> = slots.into_iter().map(|inner| self.hub.clone().close_slot(inner)).collect();
        if closes.is_empty() {
            return;
        }
        self.run_lifecycle(async move {
            for close in closes {
                close.await;
            }
        });
    }

    /// The call `call_id` rings a background account: that account opens first (the one open
    /// now goes to the background), so answering it happens where the call screen shows.
    pub(crate) fn open_for_call(&self, call_id: &str) -> Result<(), Error> {
        if self.open().has_call(call_id) {
            return Ok(());
        }
        if let Some((id, _)) = self.hub.woken_with_call(call_id) {
            let closing = self.hub.switch_to(&id)?;
            self.close_slots(closing);
        }
        Ok(())
    }

    /// The slot that has the call `call_id`: a background one if it rings there, else the open one.
    pub(crate) fn slot_with_call(&self, call_id: &str) -> Arc<Inner> {
        self.hub.woken_with_call(call_id).map(|(_, i)| i).unwrap_or_else(|| self.open())
    }
}
