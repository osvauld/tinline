//! Device lists: learning, accepting and publishing them.

use super::*;

impl NodeCtx {
    /// Another install of this account that is not removed (it may read our blobs).
    pub fn is_own_device(&self, dev: &[u8; 32]) -> bool {
        let s = self.shared.lock();
        s.me.as_ref().is_some_and(|m| m.device != *dev) && s.state.registry.iter().any(|e| e.device == *dev && !e.removed)
    }

    /// The caller's device and relay as of this call, so our next call to them dials straight
    /// there. A device we have not seen before resets `verified`, exactly as when they add us.
    /// A contact whose signed device list we hold is dialled by that list only: a device missing
    /// from it is accepted for this call (same DID) but not added.
    pub fn note_device(self: &Arc<Self>, epoch: u64, did: &str, device: [u8; 32], relay: Option<String>) {
        let changed = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return;
            }
            let Some(c) = s.state.contacts.iter_mut().find(|c| c.did == did) else { return };
            let mut changed = false;
            if c.has_device(&device) {
                if let Some(d) = c.devices.iter_mut().find(|d| d.device == device)
                    && relay.is_some()
                    && d.relay != relay
                {
                    d.relay = relay;
                    changed = true;
                }
                if c.device_list.is_none() && c.devices.first().map(|d| d.device) != Some(device) {
                    let at = c.devices.iter().position(|d| d.device == device).unwrap_or(0);
                    let d = c.devices.remove(at);
                    c.devices.insert(0, d);
                    changed = true;
                }
            } else {
                changed = std::mem::replace(&mut c.verified, false);
                if c.device_list.is_none() {
                    c.devices.insert(0, ContactDevice { device, relay });
                    c.devices.truncate(MAX_DEVICES);
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.persist_in_background(epoch);
            self.sink.on_contacts_changed();
        }
        self.touched(epoch, did);
    }

    /// A contact's signed `DeviceList` from a hello or an accept: kept if it verifies for that
    /// DID and is newer than the one we hold. A new device in it resets `verified`.
    pub fn accept_device_list(
        self: &Arc<Self>,
        epoch: u64,
        did: &str,
        blob: &proto::SignedDeviceList,
    ) -> bool {
        let changed = {
            let mut s = self.shared.lock();
            if s.epoch != epoch {
                return false;
            }
            match s.state.contacts.iter_mut().find(|c| c.did == did) {
                Some(c) => merge_device_list(c, blob),
                None => false,
            }
        };
        if changed {
            self.persist_in_background(epoch);
            self.sink.on_contacts_changed();
        }
        changed
    }

    /// Our own `DeviceList`: every live install of the registry (this device first, on its
    /// current relay). Re-signed with a higher `seq` only when its content changed.
    pub fn my_device_list(self: &Arc<Self>, me: &Me) -> Option<proto::SignedDeviceList> {
        let relay = self.endpoint().ok().as_ref().and_then(relay_of);
        let (blob, dirty) = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return None;
            }
            let label = s.device_label.clone().unwrap_or_default();
            let st = &mut s.state;
            let mut dirty = false;
            match st.registry.iter_mut().find(|e| e.device == me.device) {
                Some(e) => {
                    if relay.is_some() && e.relay != relay {
                        e.relay = relay;
                        dirty = true;
                    }
                    if !label.is_empty() && e.label != label {
                        e.label = label;
                        dirty = true;
                    }
                    e.removed = false;
                }
                None => {
                    st.registry.insert(0, OwnDevice {
                        device: me.device,
                        attestation: me.attestation.clone(),
                        label,
                        removed: false,
                        last_seen: now(),
                        relay,
                        links: Vec::new(),
                        rotated: false,
                    });
                    dirty = true;
                }
            }
            let mut want: Vec<([u8; 32], Option<String>)> = vec![(me.device, None)];
            for e in &st.registry {
                if e.device == me.device {
                    want[0].1 = e.relay.clone();
                } else if !e.removed {
                    want.push((e.device, e.relay.clone()));
                }
            }
            want.truncate(MAX_DEVICES);
            // Each listed install's recent key changes (this one's first), so a contact that
            // knew an older key keeps `verified` (packet F).
            let links: Vec<proto::Succession> = want
                .iter()
                .filter_map(|(d, _)| st.registry.iter().find(|e| e.device == *d))
                .flat_map(|e| e.links.iter().rev().cloned())
                .take(proto::MAX_LIST_LINKS)
                .collect();
            let same = st.own_list.as_ref().is_some_and(|b| {
                proto::verify_device_list(b, me.id.did()).is_ok_and(|l| {
                    l.devices.len() == want.len()
                        && l.devices
                            .iter()
                            .zip(&want)
                            .all(|(e, (d, r))| e.device == *d && &e.relay == r)
                        && l.links == links
                })
            });
            if !same {
                let prev = st
                    .own_list
                    .as_ref()
                    .and_then(|b| proto::verify_device_list(b, me.id.did()).ok())
                    .map_or(0, |l| l.seq);
                st.own_list = Some(proto::sign_device_list_with(&me.id, &want, &links, now_ms().max(prev + 1)));
                dirty = true;
            }
            (st.own_list.clone(), dirty)
        };
        if dirty {
            self.persist_in_background(me.epoch);
        }
        blob
    }
}

/// Replaces the contact's devices by `blob`'s if it is a newer list for their DID
/// (`account_model::devices::merge_device_list`, with our relay hint parsing). Returns whether
/// anything changed.
pub fn merge_device_list(c: &mut StoredContact, blob: &proto::SignedDeviceList) -> bool {
    account_model::devices::merge_device_list(c, blob, relay_hint)
}
