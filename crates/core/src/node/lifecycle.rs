//! `Node` API: start, stop, network change, status and the contact ticket.

use super::*;


#[uniffi::export]
impl Node {
    /// Binds the endpoint and starts accepting calls. Idempotent. Accounts running in the
    /// background start too (in the background); under `KeepAll` every other account the
    /// platform has a key for (`AccountEvents::account_key`) is opened and started first.
    pub fn start(&self) -> Result<(), Error> {
        if self.hub.mode() == BackgroundMode::KeepAll {
            self.hub.open_all();
        }
        for inner in self.hub.background() {
            self.hub.handle.spawn(async move {
                if let Err(e) = inner.ctx.clone().start().await {
                    tracing::warn!("starting a background account: {e}");
                }
            });
        }
        self.run(|inner| inner.ctx.clone().start())
    }

    /// Stops every endpoint, the background accounts' too (they stay unlocked; under `OnWake`
    /// they close once the idle time has passed).
    pub fn stop(&self) {
        let slots = self.hub.all_slots();
        self.run_lifecycle(async move {
            for inner in slots {
                {
                    let _life = inner.lifecycle.lock().await;
                    inner.chat.chat_sessions_close();
                    inner.group.gcall_shutdown().await;
                    inner.close_endpoint().await;
                }
                inner.emit_status();
            }
        });
    }

    /// The platform saw the network change (wifi ↔ cellular); re-probe paths now rather than
    /// waiting for the next timeout. Every running account's endpoint.
    pub fn network_changed(&self) {
        for slot in self.hub.all_slots() {
            let ep = slot.shared.lock().endpoint.clone();
            if let Some(ep) = ep {
                slot.handle.spawn(async move { ep.network_change().await });
                slot.chat.chat_kick_pending();
                slot.sync_kick();
            }
        }
    }

    pub fn status(&self) -> NodeStatus {
        self.open().status()
    }

    /// Our contact ticket (`OSVC2:…`). Reusable: any number of people can add us with it until
    /// it expires (7 days) or `reset_ticket`; each addition is a request we accept or not.
    /// Replaced (the old one stays good until it expires) on its last day, after a rename and
    /// when our relay changes.
    pub fn my_ticket(&self) -> Result<String, Error> {
        self.run(|inner| async move { inner.ctx.ticket().await })
    }
}
