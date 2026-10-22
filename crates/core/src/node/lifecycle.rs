//! `Node` API: start, stop, network change, status and the contact ticket.

use super::*;


#[uniffi::export]
impl Node {
    /// Binds the endpoint and starts accepting calls. Idempotent.
    pub fn start(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.ctx.clone().start().await.map_err(Error::from) })?
    }

    pub fn stop(&self) {
        let inner = self.inner.clone();
        self.run_lifecycle(async move {
            {
                let _life = inner.lifecycle.lock().await;
                inner.chat_sessions_close();
                inner.gcall_shutdown().await;
                inner.close_endpoint().await;
            }
            inner.emit_status();
        });
    }

    /// The platform saw the network change (wifi ↔ cellular); re-probe paths now rather than
    /// waiting for the next timeout.
    pub fn network_changed(&self) {
        let ep = self.inner.shared.lock().endpoint.clone();
        if let Some(ep) = ep {
            self.inner.handle.spawn(async move { ep.network_change().await });
            self.inner.chat_kick_pending();
            self.inner.sync_kick();
        }
    }

    pub fn status(&self) -> NodeStatus {
        self.inner.status()
    }

    /// Our contact ticket (`OSVC2:…`). Reusable: any number of people can add us with it until
    /// it expires (7 days) or `reset_ticket`; each addition is a request we accept or not.
    /// Replaced (the old one stays good until it expires) on its last day, after a rename and
    /// when our relay changes.
    pub fn my_ticket(&self) -> Result<String, Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.ctx.ticket().await.map_err(Error::from) })?
    }
}
