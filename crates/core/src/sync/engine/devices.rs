//! Own-device liveness, dialing, probes, and unlink notifications.

use super::*;

impl Inner {
    pub(super) fn touch_device(&self, epoch: u64, device: [u8; 32]) {
        let mut s = self.shared.lock();
        if s.epoch == epoch
            && let Some(e) = s.state.registry.iter_mut().find(|e| e.device == device)
        {
            e.last_seen = now();
        }
    }

    // ---- the dial loop --------------------------------------------------------------------

    /// Started with the endpoint: dials every other live device of the registry that is owed a
    /// sync (`SelfSync::synced`) and has no session, now and then (and at once on a change or
    /// `sync_kick`), with growing pauses per failing device. Devices in sync are not dialled:
    /// a device with a change dials us.
    pub(crate) fn selfsync_started(self: &Arc<Self>) {
        let this = self.clone();
        self.handle.spawn(async move {
            loop {
                if this.shared.lock().endpoint.is_none() {
                    break;
                }
                this.dial_round();
                tokio::select! {
                    _ = tokio::time::sleep(sync_period()) => {}
                    _ = this.signal.woken() => {}
                }
            }
        });
    }

    pub(super) fn dial_round(self: &Arc<Self>) {
        let Ok(me) = self.me() else { return };
        let generation = self.signal.generation();
        let targets: Vec<([u8; 32], Option<String>)> = {
            let s = self.shared.lock();
            if s.epoch != me.epoch {
                return;
            }
            let synced = self.selfsync.synced.lock();
            s.state
                .registry
                .iter()
                .filter(|e| !e.removed && e.device != me.device)
                .filter(|e| synced.get(&e.device).is_none_or(|g| *g < generation))
                .map(|e| (e.device, e.relay.clone()))
                .collect()
        };
        for (device, relay) in targets {
            if self.selfsync.sessions.lock().contains_key(&device) {
                continue;
            }
            if self
                .selfsync
                .backoff
                .lock()
                .get(&device)
                .is_some_and(|(_, at)| Instant::now() < *at)
            {
                continue;
            }
            if !self.selfsync.dialing.lock().insert(device) {
                continue;
            }
            let this = self.clone();
            self.handle.spawn(async move {
                let r = this.clone().self_dial(device, relay).await;
                this.selfsync.dialing.lock().remove(&device);
                if r.is_err() {
                    let mut b = this.selfsync.backoff.lock();
                    let n = b.get(&device).map_or(0, |(n, _)| *n) + 1;
                    let wait = Duration::from_secs((10u64 << n.min(5)).min(300));
                    b.insert(device, (n, Instant::now() + wait));
                }
            });
        }
    }

    /// For tests: a bare `Auth` exchange with `device`, as whoever we are.
    pub(crate) async fn self_probe(self: &Arc<Self>, device: [u8; 32]) -> Result<(), Error> {
        let me = self.me()?;
        let ep = self.endpoint()?;
        let conn = tokio::time::timeout(
            DIAL_TIMEOUT,
            crate::version::connect(&ep, addr_for(&device, None)?, &crate::version::OWN),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut writer = FrameWriter(send);
        let mut reader = FrameReader::new(recv);
        writer
            .send(&SelfMsg::Auth {
                attestation: me.attestation.clone(),
                relay: None,
                unlink: false,
            })
            .await?;
        let r = tokio::time::timeout(Duration::from_secs(5), reader.recv::<SelfMsg>()).await;
        conn.close(0u32.into(), b"probe");
        match r {
            Ok(Ok(Some(SelfMsg::Auth { .. }))) => Ok(()),
            _ => Err(Error::Protocol("refused".into())),
        }
    }

    /// Tells a removed device that it is (so it drops the account), if it can be reached:
    /// authenticates as one of its own devices, sends `Unlinked`, waits briefly.
    pub(crate) async fn send_unlinked(self: &Arc<Self>, device: [u8; 32], relay: Option<String>) {
        let Ok(me) = self.me() else { return };
        let Ok(ep) = self.endpoint() else { return };
        let Ok(addr) = addr_for(&device, relay.as_deref()) else {
            return;
        };
        let Ok(Ok(conn)) = tokio::time::timeout(
            Duration::from_secs(8),
            crate::version::connect(&ep, addr, &crate::version::OWN),
        )
        .await
        else {
            return;
        };
        let run = async {
            let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
            let mut writer = FrameWriter(send);
            let mut reader = FrameReader::new(recv);
            let relay = relay_of(&ep);
            writer
                .send(&SelfMsg::Auth {
                    attestation: me.attestation.clone(),
                    relay,
                    unlink: true,
                })
                .await?;
            // Their Auth first: the order the other side expects, and it proves who answered.
            let _ = tokio::time::timeout(Duration::from_secs(5), reader.recv::<SelfMsg>()).await;
            writer.send(&SelfMsg::Unlinked).await?;
            writer.finish();
            let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
            Ok::<_, Error>(())
        };
        let _ = run.await;
        conn.close(0u32.into(), b"done");
    }
}
