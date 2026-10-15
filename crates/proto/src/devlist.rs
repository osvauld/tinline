//! Signed device lists and their ordering.

use super::*;

// ---- device list -------------------------------------------------------------------

/// Most devices one account may publish (contacts dial all of them).
pub const MAX_LIST_DEVICES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceEntry {
    /// Device key, text form like every other device on the wire.
    pub device: String,
    /// Unsigned-in-spirit routing hint; a bad one is dropped on verify, the device is kept.
    pub relay: Option<String>,
}

impl DeviceEntry {
    pub fn device_key(&self) -> Result<[u8; 32]> {
        dec32(&self.device)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceList {
    pub v: u8,
    pub did: String,
    /// Unix ms chosen by the signer; higher wins (see `newer`).
    pub seq: u64,
    pub devices: Vec<DeviceEntry>,
}

/// Syntactic relay hint check. The core additionally parses it as a `RelayUrl`.
pub fn valid_relay_hint(h: &str) -> bool {
    h.len() <= 200
        && h.len() > "https://".len()
        && h.starts_with("https://")
        && !h.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Sign the account's device set. Callers keep `devices.len() <= MAX_LIST_DEVICES`.
pub fn sign_device_list(
    id: &Identity,
    devices: &[([u8; 32], Option<String>)],
    seq: u64,
) -> SignedBlob {
    let claim = DeviceList {
        v: VERSION,
        did: id.did().to_string(),
        seq,
        devices: devices
            .iter()
            .map(|(d, r)| DeviceEntry {
                device: enc(d),
                relay: r.clone(),
            })
            .collect(),
    };
    sign_blob(id, DEVICES_DOMAIN, &claim)
}

pub fn verify_device_list(blob: &SignedBlob, expected_did: &str) -> Result<DeviceList> {
    let mut list: DeviceList = open_blob(blob, DEVICES_DOMAIN, |c: &DeviceList| &c.did)?;
    check_version(list.v)?;
    if list.did != expected_did {
        return Err(Error::WrongIssuer);
    }
    if list.devices.len() > MAX_LIST_DEVICES {
        return Err(Error::TooManyDevices);
    }
    if list.devices.is_empty() {
        return Err(Error::Decode);
    }
    let mut seen = HashSet::new();
    for e in &mut list.devices {
        if !seen.insert(e.device_key()?) {
            return Err(Error::Decode);
        }
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

/// The claims of a device list WITHOUT checking anything: only to learn which DID it names, so the
/// caller can then `verify_device_list` against that DID.
pub fn peek_device_list(blob: &SignedBlob) -> Option<DeviceList> {
    serde_json::from_slice(&dec(&blob.payload).ok()?).ok()
}

/// Whether `a` should replace `b`. Both must already have passed `verify_device_list` for the
/// same DID. Higher `seq` wins; on a tie the lexicographically larger signature does, so every
/// receiver picks the same list. An unreadable blob loses.
pub fn newer(a: &SignedBlob, b: &SignedBlob) -> bool {
    #[derive(Deserialize)]
    struct S {
        seq: u64,
    }
    let key = |x: &SignedBlob| -> Option<(u64, Vec<u8>)> {
        let seq = serde_json::from_slice::<S>(&dec(&x.payload).ok()?)
            .ok()?
            .seq;
        Some((seq, dec(&x.signature).ok()?))
    };
    match (key(a), key(b)) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
    }
}
