//! The account-wide Loro doc that own devices sync (`tinline/self/1`), and the merge rules
//! between it and the working copy (`State`). The call log is its own doc (`callsdoc.rs`).
//!
//! ```text
//! devices/<device_text>  { att, label, removed, relay }   the registry
//! contacts/<did>         { rec, alias, verified }         rec = name, devices, device_list, grant, added_at
//! tomb/<did>             unix ms the contact was removed (= blocked)
//! redeemed/<nonce>       "1"                              set, never shrinks
//! revoked/<grant id>     "1"                              set, never shrinks
//! calls/<call_id>        (written by older builds, no longer read: see `callsdoc.rs`)
//! ```
//!
//! `State` stays the working copy. Every local write is flushed into the doc (`write_from`);
//! every received batch is imported and merged back (`read_into`) under the same lock, so the
//! working copy always contains what the doc says and a missing entry means a local deletion.
//! All own devices hold the identity secret, so they trust each other fully; the merge rules
//! below are about convergence, not defence.

use std::collections::{BTreeMap, HashSet};

use std::ops::Deref;

use loro::{Container, LoroMap, LoroValue, ValueOrContainer};
use serde::{Deserialize, Serialize};

use crate::node::{grant_outlives, merge_device_list};
use super::doc::{Doc, entries, field, io, text};
use crate::store::{ContactDevice, OwnDevice, State, StoredContact};
use crate::Error;

pub const DOC_NAME: &str = "account";

#[derive(Serialize, Deserialize, PartialEq, Clone)]
struct CRec {
    name: String,
    devices: Vec<ContactDevice>,
    device_list: Option<proto::SignedBlob>,
    grant: proto::SignedGrant,
    added_at: u64,
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
struct Verified {
    v: bool,
    /// `seq` of the contact's device list when it was set; a larger local list voids it (S4).
    seq: u64,
}

pub struct AccDoc(Doc);

impl Deref for AccDoc {
    type Target = Doc;
    fn deref(&self) -> &Doc {
        &self.0
    }
}

#[derive(Default, Debug)]
pub struct Changes {
    pub contacts: bool,
    pub devices: bool,
    /// Contacts the other device removed (their chats are purged here).
    pub removed: Vec<String>,
    /// Another device renamed this one.
    pub label: Option<String>,
    /// This device is tombstoned in the registry.
    pub self_removed: bool,
}

pub struct Ctx<'a> {
    pub my_did: &'a str,
    pub me_device: [u8; 32],
    pub now: u64,
    pub now_ms: i64,
}





fn int(v: &LoroValue) -> Option<i64> {
    match v {
        LoroValue::I64(n) => Some(*n),
        _ => None,
    }
}

fn sub(map: &LoroMap, key: &str) -> Result<LoroMap, Error> {
    match map.get(key) {
        Some(ValueOrContainer::Container(Container::Map(m))) => Ok(m),
        _ => map.insert_container(key, LoroMap::new()).map_err(io),
    }
}

fn set_str(m: &LoroMap, k: &str, v: &str) -> Result<(), Error> {
    if let Some(ValueOrContainer::Value(LoroValue::String(s))) = m.get(k)
        && s.as_str() == v
    {
        return Ok(());
    }
    m.insert(k, v).map_err(io)
}

fn has_key(m: &LoroMap, k: &str) -> bool {
    m.get(k).is_some()
}

fn list_seq(did: &str, blob: &Option<proto::SignedBlob>) -> u64 {
    blob.as_ref().and_then(|b| proto::verify_device_list(b, did).ok()).map_or(0, |l| l.seq)
}

impl AccDoc {
    pub fn new(device: &[u8; 32]) -> Self {
        Self(Doc::new(device, DOC_NAME))
    }

    pub fn load(device: &[u8; 32], snapshot: Option<&[u8]>) -> Result<Self, Error> {
        Doc::load(device, DOC_NAME, snapshot).map(Self)
    }

    // ---- State -> doc -----------------------------------------------------------------

    /// Flushes the working copy into the doc; whether anything changed.
    pub fn write_from(&self, st: &State, ctx: &Ctx) -> Result<bool, Error> {
        let before = self.vv();

        let devices = self.doc.get_map("devices");
        for e in &st.registry {
            let m = sub(&devices, &proto::device_to_text(&e.device))?;
            set_str(&m, "att", &serde_json::to_string(&e.attestation).map_err(io)?)?;
            set_str(&m, "label", &e.label)?;
            set_str(&m, "relay", e.relay.as_deref().unwrap_or(""))?;
            // A tombstone is never taken back.
            if e.removed {
                set_str(&m, "removed", "1")?;
            } else if !has_key(&m, "removed") {
                set_str(&m, "removed", "0")?;
            }
        }

        let contacts = self.doc.get_map("contacts");
        let tomb = self.doc.get_map("tomb");
        let live: HashSet<&str> = st.contacts.iter().map(|c| c.did.as_str()).collect();
        for c in &st.contacts {
            let m = sub(&contacts, &c.did)?;
            let rec = CRec {
                name: c.name.clone(),
                devices: c.devices.clone(),
                device_list: c.device_list.clone(),
                grant: c.grant_from_them.clone(),
                added_at: c.added_at,
            };
            set_str(&m, "rec", &serde_json::to_string(&rec).map_err(io)?)?;
            set_str(&m, "alias", &serde_json::to_string(&c.alias).map_err(io)?)?;
            let ver = Verified { v: c.verified, seq: list_seq(&c.did, &c.device_list) };
            set_str(&m, "verified", &serde_json::to_string(&ver).map_err(io)?)?;
        }
        // A contact the doc has and the working copy lacks was removed here.
        if let LoroValue::Map(cm) = contacts.get_deep_value() {
            for (did, _) in cm.iter() {
                if !live.contains(did.as_str()) {
                    contacts.delete(did).map_err(io)?;
                }
            }
        }
        for did in &st.blocked {
            if !has_key(&tomb, did) {
                tomb.insert(did, ctx.now_ms).map_err(io)?;
            }
        }
        if let LoroValue::Map(tm) = tomb.get_deep_value() {
            for (did, _) in tm.iter() {
                // Blocked ones stay; a lifted block (the contact was added again) goes.
                if !st.blocked.contains(did.as_str()) && live.contains(did.as_str()) {
                    tomb.delete(did).map_err(io)?;
                }
            }
        }

        for (name, set) in [("redeemed", &st.redeemed), ("revoked", &st.revoked)] {
            let m = self.doc.get_map(name);
            for k in set {
                if !has_key(&m, k) {
                    m.insert(k, "1").map_err(io)?;
                }
            }
        }

        self.doc.commit();
        Ok(self.vv() != before)
    }

    // ---- doc -> State -----------------------------------------------------------------

    pub fn read_into(&self, st: &mut State, ctx: &Ctx) -> Changes {
        let mut ch = Changes::default();
        let root = self.doc.get_deep_value();
        let sect = |n: &str| field(&root, n).map(entries).unwrap_or_default();

        // -- registry
        let before = serde_json::to_string(&st.registry).unwrap_or_default();
        for (dev_text, m) in sect("devices") {
            let Ok(device) = proto::device_from_text(&dev_text) else { continue };
            let Some(att) = field(m, "att").and_then(text).and_then(|t| serde_json::from_str::<proto::SignedAttestation>(&t).ok()) else { continue };
            let Ok(a) = proto::verify_attestation(&att) else { continue };
            if a.did != ctx.my_did || a.device_key().ok() != Some(device) {
                continue;
            }
            let label = field(m, "label").and_then(text).unwrap_or_default();
            let removed = field(m, "removed").and_then(text).is_some_and(|t| t == "1");
            let relay = field(m, "relay").and_then(text).filter(|r| !r.is_empty() && proto::valid_relay_hint(r));
            match st.registry.iter_mut().find(|e| e.device == device) {
                Some(e) => {
                    if device == ctx.me_device {
                        if !label.is_empty() && e.label != label {
                            e.label = label.clone();
                            ch.label = Some(label);
                        }
                    } else {
                        if !label.is_empty() {
                            e.label = label;
                        }
                        if relay.is_some() {
                            e.relay = relay;
                        }
                    }
                    e.removed |= removed;
                }
                None => st.registry.push(OwnDevice {
                    device,
                    attestation: att,
                    label,
                    removed,
                    last_seen: 0,
                    relay: if device == ctx.me_device { None } else { relay },
                }),
            }
        }
        ch.self_removed = st.registry.iter().any(|e| e.device == ctx.me_device && e.removed);
        ch.devices = serde_json::to_string(&st.registry).unwrap_or_default() != before;

        // -- contacts
        let before = (serde_json::to_string(&st.contacts).unwrap_or_default(), st.blocked.clone());
        let tombs: BTreeMap<String, i64> = sect("tomb").into_iter().filter_map(|(k, v)| Some((k, int(v)?))).collect();
        let mut doc_alive: HashSet<String> = HashSet::new();
        for (did, m) in sect("contacts") {
            let Some(rec) = field(m, "rec").and_then(text).and_then(|t| serde_json::from_str::<CRec>(&t).ok()) else { continue };
            if did == ctx.my_did {
                continue;
            }
            // A removal stands unless the contact was added again afterwards.
            if tombs.get(&did).is_some_and(|t| (rec.added_at as i64).saturating_mul(1000) <= *t) {
                continue;
            }
            doc_alive.insert(did.clone());
            let alias: Option<String> = field(m, "alias").and_then(text).and_then(|t| serde_json::from_str(&t).ok()).flatten();
            let ver: Option<Verified> = field(m, "verified").and_then(text).and_then(|t| serde_json::from_str(&t).ok());
            st.blocked.remove(&did);
            match st.contacts.iter_mut().find(|c| c.did == did) {
                None => {
                    let verified = ver.as_ref().is_some_and(|v| v.v && v.seq >= list_seq(&did, &rec.device_list));
                    st.contacts.push(StoredContact {
                        did: did.clone(),
                        name: rec.name,
                        devices: rec.devices,
                        device_list: rec.device_list,
                        grant_from_them: rec.grant,
                        added_at: rec.added_at,
                        alias,
                        verified,
                    })
                }
                Some(c) => {
                    c.name = rec.name;
                    if rec.added_at > c.added_at {
                        c.added_at = rec.added_at;
                    }
                    // Device set: the newer signed list wins; without lists, union (ours first).
                    if let Some(theirs) = &rec.device_list {
                        merge_device_list(c, theirs);
                    } else if c.device_list.is_none() {
                        for d in rec.devices {
                            if !c.has_device(&d.device) {
                                c.devices.push(d);
                            }
                        }
                        c.devices.truncate(proto::MAX_LIST_DEVICES);
                    }
                    if grant_outlives(&c.grant_from_them, &rec.grant, &did, ctx.my_did, ctx.now) {
                        c.grant_from_them = rec.grant;
                    }
                    c.alias = alias;
                    // S4: someone's "verified" counts only against a device set at least as
                    // new as ours; a device we learned of since voids it.
                    if let Some(v) = ver {
                        c.verified = v.v && v.seq >= list_seq(&did, &c.device_list);
                    }
                }
            }
        }
        // Contacts the doc no longer vouches for: removed (and blocked) on another device.
        let gone: Vec<String> = st
            .contacts
            .iter()
            .filter(|c| !doc_alive.contains(&c.did) && tombs.contains_key(&c.did))
            .map(|c| c.did.clone())
            .collect();
        for did in gone {
            st.contacts.retain(|c| c.did != did);
            st.blocked.insert(did.clone());
            ch.removed.push(did);
        }
        for did in tombs.keys() {
            if !doc_alive.contains(did) && !st.contacts.iter().any(|c| &c.did == did) {
                st.blocked.insert(did.clone());
            }
        }
        for (name, set) in [("redeemed", &mut st.redeemed), ("revoked", &mut st.revoked)] {
            for (k, _) in sect(name) {
                set.insert(k);
            }
        }
        ch.contacts = (serde_json::to_string(&st.contacts).unwrap_or_default(), st.blocked.clone()) != before || !ch.removed.is_empty();

        ch
    }
}
