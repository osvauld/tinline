//! Short invite codes (G3): the link format and the host's code rows. A host keeps the full
//! signed invite under a random code; the link carries only what is needed to reach that ONE host
//! and ask for it:
//!
//! ```text
//! tinline://g/<base58( 0x01 ‖ host group endpoint (32) ‖ code (12) ‖ relay hint )>
//! relay hint: 0x00 none | 0x01 idx (a known n0 relay) | 0x02 len name (another *.relay.n0.iroh.link)
//!             | 0x03 len url (anything else)
//! ```
//!
//! With a known n0 relay that is 47 bytes, 64-65 base58 characters, 76-77 with the prefix. The
//! bare code (without `tinline://g/`) is accepted too. Fetching, registering and revoking codes
//! over the network is p2pcore's `group/code.rs`.

use serde::{Deserialize, Serialize};
use storage::Op;

use crate::Error;
use crate::store::{GroupStore, io, json};

pub const LINK_PREFIX: &str = "tinline://g/";
pub const CODE_LEN: usize = 12;
const VERSION: u8 = 1;
const N0_SUFFIX: &str = ".relay.n0.iroh.link";
/// The n0 relays (iroh 1.3 `presets::N0`), by index.
const N0_RELAYS: [&str; 4] = ["use1-1", "usw1-1", "euc1-1", "aps1-1"];

/// What a link names: one host's group endpoint (+ relay hint) and a code it keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortCode {
    pub endpoint: [u8; 32],
    pub relay: Option<String>,
    pub code: [u8; CODE_LEN],
}

fn relay_host(url: &str) -> Option<&str> {
    let h = url.strip_prefix("https://")?;
    let h = h.strip_suffix('/').unwrap_or(h);
    Some(h.strip_suffix('.').unwrap_or(h))
}

fn pack_relay(relay: Option<&str>, out: &mut Vec<u8>) {
    let Some(url) = relay else {
        out.push(0);
        return;
    };
    if let Some(host) = relay_host(url)
        && let Some(name) = host.strip_suffix(N0_SUFFIX)
        && !name.contains('/')
    {
        if let Some(i) = N0_RELAYS.iter().position(|r| *r == name) {
            out.extend_from_slice(&[1, i as u8]);
            return;
        }
        if name.len() < 64 {
            out.extend_from_slice(&[2, name.len() as u8]);
            out.extend_from_slice(name.as_bytes());
            return;
        }
    }
    if url.len() <= 255 {
        out.extend_from_slice(&[3, url.len() as u8]);
        out.extend_from_slice(url.as_bytes());
    } else {
        out.push(0);
    }
}

fn unpack_relay(b: &[u8]) -> Option<Option<String>> {
    let n0 = |name: &str| format!("https://{name}{N0_SUFFIX}./");
    match b {
        [0] => Some(None),
        [1, i] => N0_RELAYS.get(*i as usize).map(|r| Some(n0(r))),
        [2, len, rest @ ..] if rest.len() == *len as usize => {
            let name = std::str::from_utf8(rest).ok()?;
            name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.').then(|| Some(n0(name)))
        }
        [3, len, rest @ ..] if rest.len() == *len as usize => Some(Some(std::str::from_utf8(rest).ok()?.to_string())),
        _ => None,
    }
}

impl ShortCode {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = vec![VERSION];
        b.extend_from_slice(&self.endpoint);
        b.extend_from_slice(&self.code);
        pack_relay(self.relay.as_deref(), &mut b);
        b
    }

    pub fn to_link(&self) -> String {
        format!("{LINK_PREFIX}{}", bs58::encode(self.to_bytes()).into_string())
    }

    /// A link (`tinline://g/...`, any case of the scheme, a trailing `/` tolerated) or the bare code.
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim();
        let body = match (t.get(..LINK_PREFIX.len()), t.get(LINK_PREFIX.len()..)) {
            (Some(p), Some(rest)) if p.eq_ignore_ascii_case(LINK_PREFIX) => rest,
            _ => t,
        };
        let body = body.trim_end_matches('/');
        if body.is_empty() || body.len() > 512 {
            return None;
        }
        let b = bs58::decode(body).into_vec().ok()?;
        if b.len() < 1 + 32 + CODE_LEN + 1 || b[0] != VERSION {
            return None;
        }
        let endpoint: [u8; 32] = b[1..33].try_into().ok()?;
        let code: [u8; CODE_LEN] = b[33..33 + CODE_LEN].try_into().ok()?;
        let relay = unpack_relay(&b[33 + CODE_LEN..])?;
        Some(Self { endpoint, relay, code })
    }
}

/// `CodeRow::expires` of a code that never expires (an open link). Never swept by
/// `prune_codes`; it still counts as one of the group's codes.
pub const NO_EXPIRY: u64 = u64::MAX;

/// What a code admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeKind {
    /// An admin's signed invite (bearer grant, expires with it).
    Invite,
    /// No invite: admits only the issuer's own DID (its other devices), expires like an invite.
    Device,
    /// No invite: anyone may fetch it while the group is open (`Open{active}`); never expires
    /// (`NO_EXPIRY`), dropped when the group closes (`docs/design/public-group.md` §2).
    Open,
}

/// Host side: an invite (or a device token, or an open link) kept under a code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRow {
    pub gid: String,
    pub kind: CodeKind,
    /// `codec` bytes of a `SignedInvite` (`CodeKind::Invite` only).
    pub invite: Option<Vec<u8>>,
    /// Unix ms; `NO_EXPIRY` for an open link.
    pub expires: u64,
    /// Who registered it.
    pub by: String,
}

/// Issuer side: which group and host a code we made is at (for revoking it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedRow {
    pub gid: String,
    pub host_did: String,
    pub host_dev: String,
    /// Unix ms.
    pub expires: u64,
}

impl GroupStore {
    pub fn put_code(&self, code: &[u8; CODE_LEN], r: &CodeRow) -> Result<(), Error> {
        self.db.put(&format!("code/{}", hex::encode(code)), &json(r)).map_err(io)
    }

    /// The code's row if it is unexpired (an expired one is forgotten here).
    pub fn code(&self, code: &[u8; CODE_LEN]) -> Result<Option<CodeRow>, Error> {
        let k = format!("code/{}", hex::encode(code));
        let Some(r) = self.db.get(&k).map_err(io)?.and_then(|v| serde_json::from_slice::<CodeRow>(&v).ok()) else { return Ok(None) };
        if r.expires < crdt::now_ms() {
            self.db.apply(&[Op::Delete(k)]).map_err(io)?;
            return Ok(None);
        }
        Ok(Some(r))
    }

    pub fn drop_code(&self, code: &[u8; CODE_LEN]) -> Result<(), Error> {
        self.db.apply(&[Op::Delete(format!("code/{}", hex::encode(code))), Op::Delete(format!("issued/{}", hex::encode(code)))]).map_err(io)
    }

    /// Forgets every expired code (and issued row); returns how many codes `gid` still holds.
    pub fn prune_codes(&self, gid: Option<&str>) -> Result<usize, Error> {
        let t = crdt::now_ms();
        let mut dead = Vec::new();
        let mut held = 0;
        for (k, v) in self.db.scan("code/").map_err(io)? {
            match serde_json::from_slice::<CodeRow>(&v) {
                Ok(r) if r.expires >= t => {
                    if gid == Some(r.gid.as_str()) {
                        held += 1;
                    }
                }
                _ => dead.push(Op::Delete(k)),
            }
        }
        for (k, v) in self.db.scan("issued/").map_err(io)? {
            if serde_json::from_slice::<IssuedRow>(&v).map_or(true, |r| r.expires < t) {
                dead.push(Op::Delete(k));
            }
        }
        if !dead.is_empty() {
            self.db.apply(&dead).map_err(io)?;
        }
        Ok(held)
    }

    pub fn put_issued(&self, code: &[u8; CODE_LEN], r: &IssuedRow) -> Result<(), Error> {
        self.db.put(&format!("issued/{}", hex::encode(code)), &json(r)).map_err(io)
    }

    pub fn issued(&self, code: &[u8; CODE_LEN]) -> Result<Option<IssuedRow>, Error> {
        Ok(self.db.get(&format!("issued/{}", hex::encode(code))).map_err(io)?.and_then(|v| serde_json::from_slice(&v).ok()))
    }
}

#[cfg(test)]
mod tests;
