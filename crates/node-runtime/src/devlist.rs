//! Pushing our DID-signed device list to contacts (P4; decision U8, W2-D §12 of
//! `docs/design/rotation-open-decisions.md`).
//!
//! - When the set of our installs changes (a device linked, unlinked, restored with a new device
//!   key; later an endpoint rotation) the install that sees the change re-signs `own_list`
//!   (`Inner::my_device_list`: higher seq) and queues every contact for it (`State::list_outbox`),
//!   except contacts we block and contacts that block us (U5 revised, U20). A relay change alone
//!   re-signs the list but is not pushed (PROVISIONAL (coordinator): the relay is a hint, and
//!   pushing every relay flap to every contact costs battery; contacts get it with the next call).
//! - A new contact is queued too when we have more than one install, so it can reach all of them
//!   without waiting for a call.
//! - Delivery: `Msg::Devices` to one of the contact's devices (their installs sync it among
//!   themselves), retried with backoff while our endpoint is up and again after every start, until
//!   taken. Each attempt sends the CURRENT list, so an offline contact gets only the latest.
//! - Receiving: verified for the DID it names, kept only if newer than the list held and within
//!   `proto::MAX_LIST_SEQ_AHEAD_MS` of our clock (`node::merge_device_list`). A DID we block is
//!   answered with our notice instead.
//!
//! Known limits (not decided here): a sibling that has not yet synced a block still pushes to the
//! blocked contact (§12 Q4: accepted window, as with tokens); concurrent edits by two installs
//! resolve by `proto::newer` (§12 Q2, G-FORK); a list naming a new device still clears the
//! contact's `verified` flag (§12 Q5). Any phrase holder can sign a list (shared mnemonic).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use proto::{Msg, SignedDeviceList as SignedBlob};

use crate::Error;
use crate::{NodeCtx, addr_for};
use crate::wire::{Ctrl, RETRY_SECS};

/// Runtime state that is not persisted.
#[derive(Default)]
pub struct DevlistRt {
    /// DIDs with a list delivery task running.
    delivering: Mutex<HashSet<String>>,
}

/// How one delivery attempt to one device ended.
pub enum Outcome {
    /// `DevicesTaken`: done with this list.
    Taken,
    /// They block us (their notice, now taken) or refuse us: stop sending.
    Final,
}

fn keys_of(blob: &SignedBlob, did: &str) -> Vec<String> {
    let mut v: Vec<String> = proto::verify_device_list(blob, did)
        .map(|l| {
            l.devices
                .iter()
                .map(|e| proto::device_to_text(&e.device))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

impl NodeCtx {
    /// Our set of installs may have changed: re-sign the list if needed and, if its device set
    /// differs from the last one pushed, queue it for every contact we may tell (U8).
    pub fn devices_changed(self: &Arc<Self>) {
        let Ok(me) = self.me() else { return };
        let Some(blob) = self.my_device_list(&me) else { return };
        let keys = keys_of(&blob, me.id.did());
        let queued: Vec<String> = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || keys.is_empty() || s.state.pushed_list_keys == keys {
                return;
            }
            let st = &mut s.state;
            st.pushed_list_keys = keys;
            let dids: Vec<String> = st
                .contacts
                .iter()
                .map(|c| c.did.clone())
                .filter(|d| !st.blocked.contains(d) && !crate::block::blocked_me(st, d))
                .collect();
            st.list_outbox.extend(dids.iter().cloned());
            dids
        };
        self.persist_in_background(me.epoch);
        if !queued.is_empty() {
            self.log(format!("device list changed: queued for {} contacts", queued.len()));
        }
        for did in queued {
            self.lists_kick(did);
        }
    }

    /// A contact was just added: with more than one install, send it the whole list now.
    pub fn list_for_new_contact(self: &Arc<Self>, did: &str) {
        if let Some(h) = self.hooks() {
            h.profile_for_new_contact(did);
        }
        let Ok(me) = self.me() else { return };
        let Some(blob) = self.my_device_list(&me) else { return };
        if keys_of(&blob, me.id.did()).len() < 2 {
            return;
        }
        {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || s.state.blocked.contains(did) {
                return;
            }
            s.state.list_outbox.insert(did.to_string());
        }
        self.persist_in_background(me.epoch);
        self.lists_kick(did.to_string());
    }

    /// An incoming connection that opened with `Devices`: keep the list if it is a contact's,
    /// valid, newer and within the seq bound; answer `DevicesTaken` either way, or our notice to
    /// a DID we block.
    pub async fn devices_incoming(self: &Arc<Self>, mut ctrl: Ctrl, list: SignedBlob) -> Result<(), Error> {
        let me = self.me()?;
        let did = list.peek().map(|l| l.did).unwrap_or_default();
        if let Some(notice) = self.refusal_for(&did) {
            self.log("refused a device list: blocked");
            ctrl.send(&Msg::Notice { notice }).await?;
            ctrl.finish();
            return Ok(());
        }
        if self.accept_device_list(me.epoch, &did, &list) {
            self.log(format!("device list taken from {did}"));
        } else {
            self.log(format!("device list from {did} not taken"));
        }
        ctrl.send(&Msg::DevicesTaken).await?;
        ctrl.finish();
        Ok(())
    }

    /// Delivers our current list to `did` while it is in `list_outbox`, retrying with backoff
    /// while the endpoint is up. One task per DID.
    pub fn lists_kick(self: &Arc<Self>, did: String) {
        if !self.devlist.delivering.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            // A just-started endpoint (after a restore) has no relay yet: give it a moment, so the
            // list names it and the contact need not wait for DNS discovery.
            if let Ok(ep) = this.endpoint() {
                for _ in 0..50 {
                    if crate::relay_of(&ep).is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            let mut round = 0usize;
            loop {
                let Ok(me) = this.me() else { break };
                let Ok(ep) = this.endpoint() else { break };
                let devices = {
                    let mut s = this.shared.lock();
                    if s.epoch != me.epoch {
                        break;
                    }
                    let st = &mut s.state;
                    // Not before they hold us as a contact: an undelivered Welcome kicks this again.
                    if !st.list_outbox.contains(&did) || st.welcome_outbox.contains_key(&did) {
                        break;
                    }
                    let blocked = st.blocked.contains(&did) || crate::block::blocked_me(st, &did);
                    match st.contacts.iter().find(|c| c.did == did).filter(|_| !blocked) {
                        Some(c) => c.devices.clone(),
                        None => {
                            st.list_outbox.remove(&did);
                            break;
                        }
                    }
                };
                let Some(list) = this.my_device_list(&me) else { break };
                let mut outcome = None;
                for d in &devices {
                    match this.deliver_list(&ep, &did, d.device, d.relay.as_deref(), &list).await {
                        Ok(o) => {
                            outcome = Some(o);
                            break;
                        }
                        Err(e) => tracing::debug!("device list to {did}: {e}"),
                    }
                }
                if let Some(o) = outcome {
                    let done = {
                        let mut s = this.shared.lock();
                        // A newer list signed meanwhile goes out on the next round.
                        let same = matches!(o, Outcome::Final) || s.state.own_list.as_ref() == Some(&list);
                        if same {
                            s.state.list_outbox.remove(&did);
                        }
                        same
                    };
                    this.persist_logged(me.epoch).await;
                    if matches!(o, Outcome::Taken) {
                        this.log(format!("device list delivered to {did}"));
                        this.touched(me.epoch, &did);
                    }
                    if done {
                        break;
                    }
                    round = 0;
                    continue;
                }
                if devices.is_empty() {
                    break;
                }
                tokio::time::sleep(crate::wire::retry_pause(&RETRY_SECS, round)).await;
                round += 1;
            }
            this.devlist.delivering.lock().remove(&did);
        });
    }

    pub async fn deliver_list(
        self: &Arc<Self>,
        ep: &iroh::Endpoint,
        did: &str,
        device: [u8; 32],
        relay: Option<&str>,
        list: &SignedBlob,
    ) -> Result<Outcome, Error> {
        let conn = crate::wire::dial_call(ep, addr_for(&device, relay)?).await?;
        let reply = crate::wire::exchange(conn, &Msg::Devices { list: list.clone() }).await?;
        match reply {
            Some(Msg::DevicesTaken) => Ok(Outcome::Taken),
            Some(Msg::Notice { notice }) if self.is_block_from(did, &notice) => {
                let _ = self.take_notice(&notice);
                Ok(Outcome::Final)
            }
            // Not (or no longer) their contact: nothing to retry. PROVISIONAL (coordinator).
            Some(Msg::Reject { .. }) => Ok(Outcome::Final),
            other => Err(Error::Protocol(format!("device list not taken: {other:?}"))),
        }
    }

    /// Before another exchange with `did` on `device` (a chat dial): if they are owed our list,
    /// deliver it first, so a contact that has not heard of our rotated key yet learns it from the
    /// list (and keeps `verified`) rather than meet the new key unannounced. One attempt.
    pub async fn list_before(self: &Arc<Self>, ep: &iroh::Endpoint, did: &str, device: [u8; 32], relay: Option<&str>) {
        let Ok(me) = self.me() else { return };
        let owed = {
            let s = self.shared.lock();
            s.state.list_outbox.contains(did) && !s.state.welcome_outbox.contains_key(did)
        };
        if !owed {
            return;
        }
        let Some(list) = self.my_device_list(&me) else { return };
        let Ok(o) = self.deliver_list(ep, did, device, relay, &list).await else { return };
        {
            let mut s = self.shared.lock();
            if s.epoch == me.epoch && (matches!(o, Outcome::Final) || s.state.own_list.as_ref() == Some(&list)) {
                s.state.list_outbox.remove(did);
            }
        }
        self.persist_in_background(me.epoch);
        if matches!(o, Outcome::Taken) {
            self.log(format!("device list delivered to {did}"));
            self.touched(me.epoch, did);
        }
    }

    /// After start: deliver whatever is still owed.
    pub fn lists_started(self: &Arc<Self>) {
        let dids: Vec<String> = self.shared.lock().state.list_outbox.iter().cloned().collect();
        for did in dids {
            self.lists_kick(did);
        }
    }
}

