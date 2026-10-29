//! The callee's dial-back after a `call` wake (`docs/push.md` "Calls", decided 2026-10-28).
//!
//! A woken callee that is running dials the caller's devices on the calls ALPN and sends
//! `CallReady`; the device that is calling us answers on that stream with its `CallHello`, and
//! the call runs on this connection exactly as if the caller had dialled us (`incoming_call`).
//! The other devices answer `Reject`. Both sides dial meanwhile; whichever connection carries the
//! `CallHello` first is the call (the caller's leg takes only one). This covers a caller whose
//! own dials do not get through (no live path, an outdated relay hint for us) while ours do.
//!
//! The dial-back lasts as long as the wake ring: it stops once the ring is consumed (the call
//! arrived, either way) or ends.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use proto::{Msg, SignedAttestation};

use crate::push::{CALL_ATTEMPT_MS, CALL_REDIAL_MS};
use crate::store::ContactDevice;
use crate::util::msg_name;
use crate::wire::Ctrl;
use crate::{Connection, Error, LINGER, NodeCtx, REFUSED, addr_for};

/// How long a dial-back waits for the caller's answer to `CallReady` (it answers at once).
const READY_REPLY: Duration = Duration::from_secs(5);

/// Contacts whose wake ring wants a dial-back once the endpoint is up.
#[derive(Default)]
pub struct CallBacks(parking_lot::Mutex<BTreeSet<String>>);

/// One dial-back attempt's outcome.
enum Back {
    /// The caller sent its `CallHello`: the call runs on this connection.
    Hello(Connection, Ctrl, Msg),
    /// This device of the caller is not calling us (`Reject`): stop dialling it.
    NotCalling,
}

impl NodeCtx {
    /// A call wake opened a ring for `did`: dial their devices back (now, or once started).
    pub(super) fn call_back(self: &Arc<Self>, did: &str) {
        let devices = {
            let s = self.shared.lock();
            if s.endpoint.is_none() {
                drop(s);
                self.push.call_backs.0.lock().insert(did.to_string());
                return;
            }
            s.state.contacts.iter().find(|c| c.did == did).map(|c| c.devices.clone()).unwrap_or_default()
        };
        for dev in devices {
            let (this, did) = (self.clone(), did.to_string());
            self.handle.spawn(async move { this.call_back_device(did, dev).await });
        }
    }

    /// After start: the dial-backs of rings that opened while the node was down.
    pub(super) fn call_backs_started(self: &Arc<Self>) {
        let queued = std::mem::take(&mut *self.push.call_backs.0.lock());
        for did in queued.into_iter().filter(|d| self.wake_ringing(d)) {
            self.call_back(&did);
        }
    }

    /// Dials one device of the caller every [`CALL_REDIAL_MS`] while the ring is open.
    async fn call_back_device(self: Arc<Self>, did: String, dev: ContactDevice) {
        loop {
            if !self.wake_ringing(&did) {
                return;
            }
            let attempt = tokio::time::Instant::now();
            let limit = Duration::from_millis(CALL_ATTEMPT_MS) + READY_REPLY;
            match tokio::time::timeout(limit, self.call_back_once(&dev)).await {
                Ok(Ok(Back::Hello(conn, ctrl, hello))) => {
                    let remote = dev.device;
                    let Ok(me) = self.me() else { return };
                    let permit = match self.reserved.clone().try_acquire_owned().or_else(|_| self.pending.clone().try_acquire_owned()) {
                        Ok(p) => p,
                        Err(_) => {
                            conn.close(0u32.into(), b"busy");
                            return;
                        }
                    };
                    tracing::info!("call: dial-back reached a device of {}", crate::short(&did));
                    if let Err(e) = self.clone().incoming_call(conn, ctrl, hello, remote, &me, permit).await {
                        tracing::debug!("call: dial-back call ended: {e}");
                    }
                    return;
                }
                Ok(Ok(Back::NotCalling)) => return,
                Ok(Err(e)) => tracing::debug!("call: dial-back to a device of {} failed: {e}", crate::short(&did)),
                Err(_) => tracing::debug!("call: dial-back to a device of {} timed out", crate::short(&did)),
            }
            tokio::time::sleep_until(attempt + Duration::from_millis(CALL_REDIAL_MS)).await;
        }
    }

    async fn call_back_once(&self, dev: &ContactDevice) -> Result<Back, Error> {
        let ep = self.endpoint()?;
        let me = self.me()?;
        let addr = addr_for(&dev.device, dev.relay.as_deref())?;
        let conn = tokio::time::timeout(Duration::from_millis(CALL_ATTEMPT_MS), crate::version::connect(&ep, addr, &crate::version::CALL))
            .await
            .map_err(|_| Error::Timeout)??;
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut ctrl = Ctrl::new(send, recv);
        ctrl.send(&Msg::CallReady { attestation: me.attestation.clone() }).await?;
        let reply = tokio::time::timeout(READY_REPLY, ctrl.recv()).await.map_err(|_| Error::Timeout)??;
        match reply {
            Some(hello @ Msg::CallHello { .. }) => Ok(Back::Hello(conn, ctrl, hello)),
            Some(Msg::Reject { .. }) => {
                conn.close(0u32.into(), b"bye");
                Ok(Back::NotCalling)
            }
            Some(other) => {
                conn.close(0u32.into(), b"bye");
                Err(Error::Protocol(format!("unexpected reply to CallReady: {}", msg_name(&other))))
            }
            None => Err(Error::eof("closed before the reply to CallReady")),
        }
    }

    /// A contact's `CallReady` (we called them and woke them): hand the connection to our leg
    /// dialling that device, which sends its `CallHello` on this stream. Anything else: `Reject`.
    pub(crate) async fn call_ready_incoming(&self, conn: Connection, ctrl: Ctrl, remote: [u8; 32], attestation: SignedAttestation) -> Result<(), Error> {
        let leg = proto::verify_attestation(&attestation)
            .ok()
            .filter(|a| a.device == remote)
            .and_then(|a| self.live_calls_with(&a.did).into_iter().find_map(|c| c.dial_backs.lock().remove(&remote)));
        let refused = match leg {
            Some(tx) => match tx.send((conn, ctrl)) {
                Ok(()) => return Ok(()),
                // The leg stopped meanwhile.
                Err(back) => back,
            },
            None => (conn, ctrl),
        };
        let (conn, mut ctrl) = refused;
        ctrl.send(&Msg::Reject { reason: REFUSED.into() }).await?;
        ctrl.finish();
        let _ = tokio::time::timeout(LINGER, conn.closed()).await;
        Ok(())
    }
}
