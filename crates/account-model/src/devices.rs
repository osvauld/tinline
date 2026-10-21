//! Contacts' signed device lists (U8): when a newer list replaces the devices we dial.
//! p2pcore owns learning, pushing and publishing them.

use crdt::now_ms;

use crate::store::{ContactDevice, StoredContact};

/// Replaces the contact's devices by `blob`'s if it verifies for their DID, is newer than the
/// list we hold and its seq is not too far ahead of our clock (`proto::list_seq_in_bounds`,
/// PROVISIONAL (coordinator)). Hints are kept for devices we knew; a device we did not know resets
/// `verified` unless the list's links show it is a rotated key of one we knew. Returns whether
/// anything changed. `relay_hint` sanitises the list's unsigned relay hints (p2pcore parses them
/// as iroh relay URLs).
pub fn merge_device_list(c: &mut StoredContact, blob: &proto::SignedDeviceList, relay_hint: fn(&Option<String>) -> Option<String>) -> bool {
    let Ok(list) = proto::verify_device_list(blob, &c.did) else {
        return false;
    };
    if !proto::list_seq_in_bounds(list.seq, now_ms()) {
        return false;
    }
    if c.device_list.as_ref().is_some_and(|old| !proto::newer(blob, old)) {
        return false;
    }
    let devices: Vec<ContactDevice> = list
        .devices
        .iter()
        .filter_map(|e| {
            let device = e.device;
            let relay = relay_hint(&e.relay).or_else(|| c.relay_for(&device).map(String::from));
            Some(ContactDevice { device, relay })
        })
        .collect();
    if devices.is_empty() {
        return false;
    }
    // A device we did not know resets `verified`, unless the list proves it is a rotated key of
    // an install we knew: a chain of links each signed by the previous key (packet F). A new
    // install of the same DID (a link, a restore) has no such chain.
    if devices.iter().any(|d| !c.has_device(&d.device) && !proto::succeeds(&list, &d.device, |k| c.has_device(k))) {
        c.verified = false;
    }
    c.devices = devices;
    c.device_list = Some(blob.clone());
    true
}
