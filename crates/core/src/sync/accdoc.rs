//! The account-wide Loro doc that own devices sync (`tinline/self/3`), and the merge rules
//! between it and the working copy (`State`). The call log is its own doc (`callsdoc.rs`).
//!
//! ```text
//! devices/<device_text>  { att, label, removed, relay, links, rotated }   the registry
//! contacts/<did>         { rec, alias, verified, calls }  rec = name, devices, device_list, grant,
//!                                                          call_grant, added_at; calls = we let them call
//! cutoffs/<did>          our token cut-offs for <did> (JSON `CutOffs`; per field, max wins)
//! sent/<did>             our latest signed block/unblock notice to <did> (JSON; newest `at` wins)
//! notices/<did>          the latest notice <did> signed to us + their name (JSON; newest wins)
//! redeemed/<key>         "1"   <nonce>/<joiner did>; set, never shrinks
//! calls/<call_id>        (written by older builds, no longer read: see `callsdoc.rs`)
//! ```
//!
//! `State` stays the working copy. Every local write is flushed into the doc (`write_from`);
//! every received batch is imported and merged back (`read_into`) under the same lock, so the
//! working copy always contains what the doc says and a missing entry means a local deletion.
//! All own devices hold the identity secret, so they trust each other fully; the merge rules
//! below are about convergence, not defence.

use std::collections::HashSet;

use std::ops::Deref;

use loro::{Container, LoroMap, LoroValue, ValueOrContainer};
use serde::{Deserialize, Serialize};

use crate::node::merge_device_list;
use super::doc::{Doc, entries, field, io, text};
use crate::store::{ContactDevice, CutOffs, HeldNotice, OwnDevice, State, StoredContact};
use crate::Error;

pub const DOC_NAME: &str = "account";

#[derive(Serialize, Deserialize, PartialEq, Clone)]
struct CRec {
    name: String,
    devices: Vec<ContactDevice>,
    device_list: Option<proto::SignedDeviceList>,
    grant: proto::SignedGrant,
    #[serde(default)]
    call_grant: Option<proto::SignedGrant>,
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
    /// Contacts the other device deleted (a block notice's deletion request); history is kept.
    pub removed: Vec<String>,
    /// Another device renamed this one.
    pub label: Option<String>,
    /// This device is tombstoned in the registry.
    pub self_removed: bool,
}

pub struct Ctx<'a> {
    pub my_did: &'a str,
    pub me_device: [u8; 32],
    /// Unix ms.
    pub now_ms: u64,
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

fn list_seq(did: &str, blob: &Option<proto::SignedDeviceList>) -> u64 {
    blob.as_ref()
        .and_then(|b| proto::verify_device_list(b, did).ok())
        .map_or(0, |l| l.seq)
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
    pub fn write_from(&self, st: &State, _ctx: &Ctx) -> Result<bool, Error> {
        let before = self.vv();

        let devices = self.doc.get_map("devices");
        for e in &st.registry {
            let m = sub(&devices, &proto::device_to_text(&e.device))?;
            set_str(&m, "att", &serde_json::to_string(&e.attestation).map_err(io)?)?;
            set_str(&m, "label", &e.label)?;
            set_str(&m, "relay", e.relay.as_deref().unwrap_or(""))?;
            if !e.links.is_empty() {
                set_str(&m, "links", &serde_json::to_string(&e.links).map_err(io)?)?;
            }
            // A tombstone is never taken back.
            if e.removed {
                set_str(&m, "removed", "1")?;
            } else if !has_key(&m, "removed") {
                set_str(&m, "removed", "0")?;
            }
            if e.rotated {
                set_str(&m, "rotated", "1")?;
            }
        }

        let contacts = self.doc.get_map("contacts");
        let live: HashSet<&str> = st.contacts.iter().map(|c| c.did.as_str()).collect();
        for c in &st.contacts {
            let m = sub(&contacts, &c.did)?;
            let rec = CRec {
                name: c.name.clone(),
                devices: c.devices.clone(),
                device_list: c.device_list.clone(),
                grant: c.grant_from_them.clone(),
                call_grant: c.call_grant_from_them.clone(),
                added_at: c.added_at,
            };
            set_str(&m, "rec", &serde_json::to_string(&rec).map_err(io)?)?;
            set_str(&m, "alias", &serde_json::to_string(&c.alias).map_err(io)?)?;
            let ver = Verified { v: c.verified, seq: list_seq(&c.did, &c.device_list) };
            set_str(&m, "verified", &serde_json::to_string(&ver).map_err(io)?)?;
            set_str(&m, "calls", if c.calls_allowed { "1" } else { "0" })?;
        }
        // A contact the doc has and the working copy lacks was removed here.
        if let LoroValue::Map(cm) = contacts.get_deep_value() {
            for (did, _) in cm.iter() {
                if !live.contains(did.as_str()) {
                    contacts.delete(did).map_err(io)?;
                }
            }
        }
        // Notices are never deleted: a newer one (unblock after block) replaces the older.
        let sent = self.doc.get_map("sent");
        for (did, n) in &st.sent_notices {
            set_str(&sent, did, &serde_json::to_string(n).map_err(io)?)?;
        }
        // Cut-offs only rise; a merge takes the max per field.
        let cutoffs = self.doc.get_map("cutoffs");
        for (did, c) in &st.cut_offs {
            set_str(&cutoffs, did, &serde_json::to_string(c).map_err(io)?)?;
        }
        let held = self.doc.get_map("notices");
        for (did, h) in &st.notices {
            set_str(&held, did, &serde_json::to_string(h).map_err(io)?)?;
        }

        let redeemed = self.doc.get_map("redeemed");
        for k in &st.redeemed {
            if !has_key(&redeemed, k) {
                redeemed.insert(k, "1").map_err(io)?;
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
            let Ok(device) = proto::device_from_text(&dev_text) else {
                continue;
            };
            let Some(att) = field(m, "att")
                .and_then(text)
                .and_then(|t| serde_json::from_str::<proto::SignedAttestation>(&t).ok())
            else {
                continue;
            };
            let Ok(a) = proto::verify_attestation(&att) else {
                continue;
            };
            if a.did != ctx.my_did || a.device != device {
                continue;
            }
            let label = field(m, "label").and_then(text).unwrap_or_default();
            let removed = field(m, "removed").and_then(text).is_some_and(|t| t == "1");
            let rotated = field(m, "rotated").and_then(text).is_some_and(|t| t == "1");
            // Links ending at this device, each signed by the key before (checked by contacts).
            let links: Vec<proto::Succession> = field(m, "links")
                .and_then(text)
                .and_then(|t| serde_json::from_str(&t).ok())
                .filter(|l: &Vec<proto::Succession>| l.last().is_some_and(|x| x.to == device))
                .unwrap_or_default();
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
                    e.rotated |= rotated;
                    if device != ctx.me_device && e.links.len() < links.len() {
                        e.links = links;
                    }
                }
                None => st.registry.push(OwnDevice {
                    device,
                    attestation: att,
                    label,
                    removed,
                    last_seen: 0,
                    relay: if device == ctx.me_device { None } else { relay },
                    links,
                    rotated,
                }),
            }
        }
        ch.self_removed = st.registry.iter().any(|e| e.device == ctx.me_device && e.removed);
        ch.devices = serde_json::to_string(&st.registry).unwrap_or_default() != before;

        // -- contacts
        let snap = |st: &State| {
            let mut held: Vec<(String, u64)> = st.notices.iter().map(|(d, h)| (d.clone(), h.at)).collect();
            held.sort();
            (serde_json::to_string(&st.contacts).unwrap_or_default(), st.blocked.clone(), held)
        };
        let before = snap(st);
        // Notices first (every one verified here too, U10): they decide who is blocked and which
        // contact entries a deletion request removed.
        let at_of = |b: &proto::SignedBlockNotice, to: &str| {
            proto::verify_block_notice(b, to)
                .ok()
                .filter(|n| n.from == ctx.my_did)
                .map(|n| n.at)
        };
        for (did, v) in sect("sent") {
            let Some(blob) =
                text(v).and_then(|t| serde_json::from_str::<proto::SignedBlockNotice>(&t).ok())
            else {
                continue;
            };
            let Some(at) = at_of(&blob, &did) else {
                continue;
            };
            if st
                .sent_notices
                .get(&did)
                .and_then(|b| at_of(b, &did))
                .is_none_or(|mine| at > mine)
            {
                st.sent_notices.insert(did, blob);
            }
        }
        st.blocked = st
            .sent_notices
            .iter()
            .filter(|(did, b)| proto::verify_block_notice(b, did).is_ok_and(|n| n.blocked))
            .map(|(did, _)| did.clone())
            .collect();
        for (did, v) in sect("notices") {
            let Some(h) = text(v).and_then(|t| serde_json::from_str::<HeldNotice>(&t).ok()) else { continue };
            let Ok(n) = proto::verify_block_notice(&h.notice, ctx.my_did) else { continue };
            if n.from != did || st.notices.get(&did).is_some_and(|mine| mine.at >= n.at) {
                continue;
            }
            st.notices.insert(did, HeldNotice { notice: h.notice, name: h.name, blocked: n.blocked, at: n.at });
        }
        let block_at: std::collections::HashMap<String, u64> = st.notices.iter().filter(|(_, h)| h.blocked).map(|(d, h)| (d.clone(), h.at)).collect();
        let deleted = |did: &str, added_at: u64| block_at.get(did).is_some_and(|at| added_at.saturating_mul(1000) <= *at);
        let mut doc_alive: HashSet<String> = HashSet::new();
        for (did, m) in sect("contacts") {
            let Some(rec) = field(m, "rec").and_then(text).and_then(|t| serde_json::from_str::<CRec>(&t).ok()) else { continue };
            if did == ctx.my_did {
                continue;
            }
            // A deletion request stands unless the contact was added again afterwards.
            if deleted(&did, rec.added_at) {
                continue;
            }
            doc_alive.insert(did.clone());
            let alias: Option<String> = field(m, "alias").and_then(text).and_then(|t| serde_json::from_str(&t).ok()).flatten();
            let ver: Option<Verified> = field(m, "verified").and_then(text).and_then(|t| serde_json::from_str(&t).ok());
            let calls: Option<bool> = field(m, "calls").and_then(text).map(|t| t != "0");
            match st.contacts.iter_mut().find(|c| c.did == did) {
                None => {
                    let verified = ver.as_ref().is_some_and(|v| v.v && v.seq >= list_seq(&did, &rec.device_list));
                    st.contacts.push(StoredContact {
                        did: did.clone(),
                        name: rec.name,
                        devices: rec.devices,
                        device_list: rec.device_list,
                        grant_from_them: rec.grant,
                        call_grant_from_them: rec.call_grant,
                        calls_allowed: calls.unwrap_or(true),
                        added_at: rec.added_at,
                        alias,
                        verified,
                    })
                }
                Some(c) => {
                    c.name = rec.name;
                    let readded_elsewhere = rec.added_at > c.added_at;
                    if readded_elsewhere {
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
                    if crate::caps::outlives(Some(&c.grant_from_them), &rec.grant, proto::Cap::Message, &did, ctx.my_did, ctx.now_ms) {
                        c.grant_from_them = rec.grant;
                    }
                    // A re-add elsewhere may have replaced the call token with none: the doc's
                    // slot wins when it is newer, else the later-expiring token.
                    if let Some(g) = rec.call_grant {
                        if crate::caps::outlives(c.call_grant_from_them.as_ref(), &g, proto::Cap::Call, &did, ctx.my_did, ctx.now_ms) {
                            c.call_grant_from_them = Some(g);
                        }
                    } else if readded_elsewhere {
                        c.call_grant_from_them = None;
                    }
                    if let Some(calls) = calls {
                        c.calls_allowed = calls;
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
        // Contacts a deletion request removed (here or on another device).
        for (did, at) in block_at {
            if crate::block::apply_block_deletion_request(st, &did, at) {
                ch.removed.push(did);
            }
        }
        for (did, v) in sect("cutoffs") {
            if let Some(c) = text(v).and_then(|t| serde_json::from_str::<CutOffs>(&t).ok()) {
                st.cut_offs.entry(did).or_default().merge(&c);
            }
        }
        for (k, _) in sect("redeemed") {
            st.redeemed.insert(k);
        }
        ch.contacts = snap(st) != before || !ch.removed.is_empty();

        ch
    }
}
