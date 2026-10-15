//! Signed device lists and their ordering.

use super::*;

// ---- device list -------------------------------------------------------------------

/// Most devices one account may publish (contacts dial all of them).
pub const MAX_LIST_DEVICES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceEntry {
    pub device: [u8; 32],
    /// Unsigned-in-spirit routing hint; a bad one is dropped on verify, the device is kept.
    pub relay: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceList {
    pub did: String,
    /// Unix ms chosen by the signer; higher wins (see `newer`).
    pub seq: u64,
    pub devices: Vec<DeviceEntry>,
}

/// Longest relay hint kept (`valid_relay_hint`); longer lists do not decode.
const MAX_RELAY_HINT: usize = 200;

impl Signable for DeviceList {
    const DOMAIN: domains::Domain = domains::DEVICES;
    const MAX_BODY: usize = 128 + MAX_LIST_DEVICES * (32 + 4 + MAX_RELAY_HINT);
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.did)
    }
    fn check(&self) -> Result<()> {
        if self.devices.len() > MAX_LIST_DEVICES {
            return Err(Error::TooManyDevices);
        }
        if self.devices.is_empty() {
            return Err(Error::Decode);
        }
        let mut seen = HashSet::new();
        if !self.devices.iter().all(|e| seen.insert(e.device)) {
            return Err(Error::Decode);
        }
        Ok(())
    }
}

pub type SignedDeviceList = Signed<DeviceList>;

/// Syntactic relay hint check. The core additionally parses it as a `RelayUrl`.
pub fn valid_relay_hint(h: &str) -> bool {
    h.len() <= MAX_RELAY_HINT
        && h.len() > "https://".len()
        && h.starts_with("https://")
        && !h.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Sign the account's device set. Callers keep `devices.len() <= MAX_LIST_DEVICES`.
pub fn sign_device_list(
    id: &Identity,
    devices: &[([u8; 32], Option<String>)],
    seq: u64,
) -> SignedDeviceList {
    let claim = DeviceList {
        did: id.did().to_string(),
        seq,
        devices: devices
            .iter()
            .map(|(d, r)| DeviceEntry {
                device: *d,
                relay: r.clone().filter(|r| valid_relay_hint(r)),
            })
            .collect(),
    };
    Signed::sign(&claim, id).expect("own device list signs")
}

pub fn verify_device_list(blob: &SignedDeviceList, expected_did: &str) -> Result<DeviceList> {
    let mut list = blob.open_from(expected_did)?;
    for e in &mut list.devices {
        if e.relay.as_deref().is_some_and(|r| !valid_relay_hint(r)) {
            e.relay = None;
        }
    }
    Ok(list)
}

/// PROVISIONAL (coordinator): a receiver refuses a device list whose `seq` (unix ms chosen by the
/// signer) is more than this far ahead of its own clock, so a phrase holder cannot pin contacts
/// with a huge seq (W2-D finding: `u64::MAX` pinned them forever). A list up to a day ahead still
/// wins until then.
pub const MAX_LIST_SEQ_AHEAD_MS: u64 = 24 * 3600 * 1000;

/// Whether a list `seq` is acceptable at the receiver's `now_ms` (see `MAX_LIST_SEQ_AHEAD_MS`).
pub fn list_seq_in_bounds(seq: u64, now_ms: u64) -> bool {
    seq <= now_ms.saturating_add(MAX_LIST_SEQ_AHEAD_MS)
}

/// Whether `a` should replace `b`. Both must already have passed `verify_device_list` for the
/// same DID. Higher `seq` wins; on a tie the lexicographically larger signature does, so every
/// receiver picks the same list. An unreadable list loses.
pub fn newer(a: &SignedDeviceList, b: &SignedDeviceList) -> bool {
    let key = |x: &SignedDeviceList| x.peek().ok().map(|l| (l.seq, *x.sig()));
    match (key(a), key(b)) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
    }
}
