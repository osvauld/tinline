//! Inviting contacts directly (G3). An admin picks contacts; each gets a fresh signed invite (the
//! same 7-day bearer grant a link carries, with the group's hosts) over the existing 1:1 private
//! channel: `proto::Msg::GroupLetter` on the calls ALPN, authenticated like a call hello (the
//! sender's attestation = the connection's device, and a token the receiver issued to the sender;
//! the receiver requires a MESSAGE token, P6), answered `GroupLetterTaken`. No protocol bump: the
//! calls message set is extended.
//!
//! ```text
//! inviter -> invitee   Invite { id, token (InviteToken with the signed invite), inviter_name }
//! invitee -> inviter   Answer { id, joined }      (Decline; "Joined" the inviter sees from the doc)
//! ```
//!
//! - The recipient lists it ("X invited you to G", `group_invitations_incoming`, events on
//!   `GroupInvitationEvents`); Join runs the normal join with the delivered token (the same
//!   verification as a link); Decline answers the inviter.
//! - The inviter sees per invitee Pending / Joined (the invitee is a member in the group's records)
//!   / Declined / Expired / Blocked.
//! - Undelivered letters wait in an outbox (`inv/{id}` rows with `owe`) and are retried with
//!   backoff while the endpoint is up and after every start, like the device-list push (P4).
//! - P5: we never invite a DID we block or that blocks us (refused locally; the app does not offer
//!   them); an invitee that blocks us answers its block notice, and the invitation reads Blocked.
//!   An invitation from a DID we block is answered with our notice and never listed.
//! - Invitations expire with their invite (7 days).
//! - Own devices: rows are synced over self/3 (`SelfMsg::Invitations`); an answer on one device
//!   shows on the others. Only the device that made a row delivers its letter.

use std::sync::Arc;

use base64::Engine;
use proto::{Msg, SignedGrant};
use serde::{Deserialize, Serialize};

use super::engine::{GroupCore, INVITE_TTL};
use super::record::*;
use super::store::{GroupStore, io, json};
use crate::Error;
use crate::node::{Inner, Me, Node, addr_for, now, now_ms};
use crate::wire::{Ctrl, RETRY_SECS};

/// Largest letter (a token with many hosts fits easily).
const MAX_LETTER: usize = 64 * 1024;
/// Rows are forgotten this long after their invite expired (PROVISIONAL (coordinator)).
const KEEP_AFTER_EXPIRY: u64 = 7 * 24 * 3600;
/// Most rows sent to an own device at once.
pub const MAX_OWN_INVITATIONS: usize = 512;
/// The self/3 dirty mark for invitations.
pub(crate) const INVITES_MARK: &str = "!invites";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum GroupInvitationState {
    /// Not answered yet.
    Pending,
    /// The invitee joined the group.
    Joined,
    Declined,
    /// The invite ran out (7 days) before it was answered.
    Expired,
    /// The invitee blocks the inviter (P5): it was never shown to them.
    Blocked,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupInvitation {
    pub id: String,
    pub group_id: String,
    pub group_name: String,
    pub inviter_did: String,
    pub inviter_name: String,
    pub invitee_did: String,
    pub invitee_name: String,
    /// We invited (`true`) or were invited.
    pub outgoing: bool,
    pub state: GroupInvitationState,
    /// Unix ms.
    pub at: u64,
    /// Unix seconds.
    pub expires: u64,
    /// Outgoing: one of the invitee's devices took it (or someone answered).
    pub delivered: bool,
}

/// Registered with `Node::set_group_invitation_events`. Called on core threads: do not block.
#[uniffi::export(rust, foreign)]
pub trait GroupInvitationEvents: Send + Sync {
    /// An invitation appeared or changed state (either direction).
    fn on_group_invitation(&self, invitation: GroupInvitation);
}

/// One invitation as this device holds it (group store `inv/{id}`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct InvRow {
    pub id: String,
    pub group: String,
    pub group_name: String,
    pub inviter: String,
    pub inviter_name: String,
    pub invitee: String,
    pub invitee_name: String,
    pub outgoing: bool,
    pub state: GroupInvitationState,
    pub at: u64,
    pub expires: u64,
    /// `codec` bytes of an `InviteToken` (with the signed invite).
    pub token: Vec<u8>,
    #[serde(default)]
    pub delivered: bool,
    /// This device still has to deliver this row's letter (the invite, or our decline).
    #[serde(default)]
    pub owe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum Letter {
    Invite { id: [u8; 16], token: Vec<u8>, inviter_name: String },
    Answer { id: [u8; 16], joined: bool },
}

fn letter_text(l: &Letter) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cryptography::codec::to_bytes(l).expect("letter serialises"))
}

fn letter_of(t: &str) -> Option<Letter> {
    if t.len() > MAX_LETTER * 2 {
        return None;
    }
    let b = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(t).ok()?;
    cryptography::codec::from_bytes_canonical(&b, MAX_LETTER).ok()
}

fn id_bytes(id: &str) -> Option<[u8; 16]> {
    hex::decode(id).ok()?.try_into().ok()
}

fn final_state(s: GroupInvitationState) -> bool {
    s != GroupInvitationState::Pending
}

impl GroupStore {
    pub(crate) fn put_inv(&self, r: &InvRow) -> Result<(), Error> {
        self.db.put(&format!("inv/{}", r.id), &json(r)).map_err(io)
    }

    pub(crate) fn inv(&self, id: &str) -> Result<Option<InvRow>, Error> {
        Ok(self.db.get(&format!("inv/{id}")).map_err(io)?.and_then(|v| serde_json::from_slice(&v).ok()))
    }

    pub(crate) fn invs(&self) -> Result<Vec<InvRow>, Error> {
        Ok(self.db.scan("inv/").map_err(io)?.into_iter().filter_map(|(_, v)| serde_json::from_slice(&v).ok()).collect())
    }

    fn drop_inv(&self, id: &str) -> Result<(), Error> {
        self.db.apply(&[storage::Op::Delete(format!("inv/{id}"))]).map_err(io)
    }
}

/// How one delivery attempt ended.
enum Outcome {
    Taken,
    /// Their block notice (taken): stop.
    Blocked,
    /// Refused (not, or no longer, their contact; token taken back): stop (PROVISIONAL).
    Refused,
}

impl Inner {
    /// The row as the app sees it: Pending turns Expired after the invite ran out, and Joined
    /// once the invitee is a member in the group's records (kept).
    pub(super) fn inv_current(&self, core: &GroupCore, mut r: InvRow) -> InvRow {
        if r.state == GroupInvitationState::Pending {
            let joined = core.groups.lock().get(&r.group).is_some_and(|l| l.state.is_member(&r.invitee) && !l.state.is_removed(&r.invitee));
            if joined {
                r.state = GroupInvitationState::Joined;
                r.owe = false;
                let _ = core.store.put_inv(&r);
            } else if r.expires < now() {
                r.state = GroupInvitationState::Expired;
            }
        }
        r
    }

    pub(super) fn inv_view(&self, r: &InvRow) -> GroupInvitation {
        GroupInvitation {
            id: r.id.clone(),
            group_id: r.group.clone(),
            group_name: r.group_name.clone(),
            inviter_did: r.inviter.clone(),
            inviter_name: r.inviter_name.clone(),
            invitee_did: r.invitee.clone(),
            invitee_name: r.invitee_name.clone(),
            outgoing: r.outgoing,
            state: r.state,
            at: r.at,
            expires: r.expires,
            delivered: r.delivered,
        }
    }

    /// Saves a row, tells the app and our other devices.
    pub(super) fn inv_save(&self, core: &GroupCore, r: &InvRow) {
        if let Err(e) = core.store.put_inv(r) {
            tracing::warn!("group invitation: {e}");
        }
        self.selfsync_dirty(INVITES_MARK);
        if let Some(ev) = self.group.inv_events.lock().clone() {
            let v = self.inv_view(r);
            self.handle.spawn_blocking(move || ev.on_group_invitation(v));
        }
    }

    /// The group changed: outgoing invitations whose invitee became a member read Joined.
    pub(crate) fn group_invitations_changed(&self, core: &GroupCore, gid: &str) {
        let Ok(rows) = core.store.invs() else { return };
        for r in rows {
            if r.group == gid && r.state == GroupInvitationState::Pending {
                let cur = self.inv_current(core, r.clone());
                if cur.state == GroupInvitationState::Joined {
                    self.inv_save(core, &cur);
                }
            }
        }
    }

    pub(super) fn inv_list(self: &Arc<Self>, filter: impl Fn(&InvRow) -> bool) -> Result<Vec<GroupInvitation>, Error> {
        let core = self.group_core()?;
        let t = now();
        let mut rows = Vec::new();
        for r in core.store.invs()? {
            if r.expires + KEEP_AFTER_EXPIRY < t {
                let _ = core.store.drop_inv(&r.id);
                continue;
            }
            if filter(&r) {
                rows.push(self.inv_current(&core, r));
            }
        }
        rows.sort_by(|a, b| b.at.cmp(&a.at));
        Ok(rows.iter().map(|r| self.inv_view(r)).collect())
    }

    // ---- delivery -----------------------------------------------------------------------------

    /// The letter a row owes, if any (an unexpired pending invite of ours, or our decline).
    fn inv_letter(r: &InvRow) -> Option<Letter> {
        let id = id_bytes(&r.id)?;
        if !r.owe {
            return None;
        }
        if r.outgoing && r.state == GroupInvitationState::Pending && r.expires >= now() {
            return Some(Letter::Invite { id, token: r.token.clone(), inviter_name: r.inviter_name.clone() });
        }
        if !r.outgoing && r.state == GroupInvitationState::Declined {
            return Some(Letter::Answer { id, joined: false });
        }
        None
    }

    /// Delivers what we owe `did` while the endpoint is up, with backoff. One task per DID.
    pub(crate) fn invites_kick(self: &Arc<Self>, did: String) {
        if !self.group.inv_delivering.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut round = 0usize;
            loop {
                let Ok(me) = this.me() else { break };
                let Ok(ep) = this.endpoint() else { break };
                let Ok(core) = this.group_core() else { break };
                let owed: Vec<InvRow> = core
                    .store
                    .invs()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|r| r.owe && (if r.outgoing { r.invitee == did } else { r.inviter == did }))
                    .collect();
                let mut todo = Vec::new();
                for mut r in owed {
                    match Self::inv_letter(&r) {
                        Some(l) => todo.push((r, l)),
                        None => {
                            r.owe = false;
                            let _ = core.store.put_inv(&r);
                        }
                    }
                }
                if todo.is_empty() {
                    break;
                }
                let contact = {
                    let s = this.shared.lock();
                    let st = &s.state;
                    let blocked = st.blocked.contains(&did) || crate::block::blocked_me(st, &did);
                    st.contacts.iter().find(|c| c.did == did).filter(|_| !blocked).map(|c| (c.devices.clone(), c.grant_from_them.clone()))
                };
                let Some((devices, grant)) = contact else {
                    // No longer a contact, or blocked meanwhile: nothing more to send.
                    for (mut r, _) in todo {
                        r.owe = false;
                        if r.outgoing && crate::block::blocked_me(&this.shared.lock().state, &did) {
                            r.state = GroupInvitationState::Blocked;
                        }
                        this.inv_save(&core, &r);
                    }
                    break;
                };
                let mut progressed = false;
                for (mut r, letter) in todo {
                    let text = letter_text(&letter);
                    let mut outcome = None;
                    for d in &devices {
                        match this.deliver_letter(&ep, &me, &did, d.device, d.relay.as_deref(), &grant, &text).await {
                            Ok(o) => {
                                outcome = Some(o);
                                break;
                            }
                            Err(e) => tracing::debug!("group invitation to {did}: {e}"),
                        }
                    }
                    let Some(o) = outcome else { continue };
                    progressed = true;
                    // Re-read: it may have been answered meanwhile.
                    if let Ok(Some(cur)) = core.store.inv(&r.id) {
                        r = cur;
                    }
                    r.owe = false;
                    match o {
                        Outcome::Taken => {
                            r.delivered = true;
                            this.log(format!("group invitation delivered to {did}"));
                        }
                        Outcome::Blocked if r.outgoing => r.state = GroupInvitationState::Blocked,
                        Outcome::Blocked | Outcome::Refused => {}
                    }
                    this.inv_save(&core, &r);
                }
                if progressed {
                    round = 0;
                    continue;
                }
                if devices.is_empty() {
                    break;
                }
                tokio::time::sleep(crate::wire::retry_pause(&RETRY_SECS, round)).await;
                round += 1;
            }
            this.group.inv_delivering.lock().remove(&did);
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn deliver_letter(
        self: &Arc<Self>,
        ep: &iroh::Endpoint,
        me: &Me,
        did: &str,
        device: [u8; 32],
        relay: Option<&str>,
        grant: &SignedGrant,
        letter: &str,
    ) -> Result<Outcome, Error> {
        let conn = crate::wire::dial_call(ep, addr_for(&device, relay)?).await?;
        let reply = crate::wire::exchange(conn, &Msg::GroupLetter { attestation: me.attestation.clone(), grant: grant.clone(), letter: letter.to_string() }).await?;
        match reply {
            Some(Msg::GroupLetterTaken) => Ok(Outcome::Taken),
            Some(Msg::Notice { notice }) if self.is_block_from(did, &notice) => {
                let _ = self.take_notice(&notice);
                Ok(Outcome::Blocked)
            }
            Some(Msg::Reject { .. }) => Ok(Outcome::Refused),
            other => Err(Error::Protocol(format!("group letter not taken: {other:?}"))),
        }
    }

    /// After start: deliver whatever is still owed.
    pub(crate) fn group_invites_started(self: &Arc<Self>) {
        let Ok(core) = self.group_core() else { return };
        let mut dids: Vec<String> = core.store.invs().unwrap_or_default().into_iter().filter(|r| r.owe).map(|r| if r.outgoing { r.invitee } else { r.inviter }).collect();
        dids.sort();
        dids.dedup();
        for d in dids {
            self.invites_kick(d);
        }
    }

    // ---- receiving ----------------------------------------------------------------------------

    /// An incoming connection that opened with `GroupLetter`: who sent it (as for a call hello),
    /// our notice to a DID we block, a message token of ours (P6), then the letter.
    pub(crate) async fn group_letter_incoming(self: &Arc<Self>, mut ctrl: Ctrl, msg: Msg, remote: [u8; 32], me: &Me) -> Result<(), Error> {
        let verified = {
            let s = self.shared.lock();
            let contacts = &s.state.contacts;
            proto::accept_hello(&me.id, &msg, remote, now(), &s.state.revoked, |did| contacts.iter().any(|c| c.did == did))
        };
        let sender = match verified {
            Ok(c) => c,
            Err(e) => {
                self.log(format!("refused a group letter: {e}"));
                ctrl.send(&Msg::Reject { reason: crate::node::REFUSED.into() }).await?;
                ctrl.finish();
                return Ok(());
            }
        };
        if let Some(notice) = self.refusal_for(&sender.did) {
            self.log("refused a group letter: blocked");
            ctrl.send(&Msg::Notice { notice }).await?;
            ctrl.finish();
            return Ok(());
        }
        if self.token_check(&sender.did, &sender.grant, proto::Cap::Message).is_err() {
            self.log("refused a group letter: token");
            ctrl.send(&Msg::Reject { reason: crate::node::REFUSED.into() }).await?;
            ctrl.finish();
            return Ok(());
        }
        let Msg::GroupLetter { letter, .. } = &msg else { unreachable!() };
        match letter_of(letter) {
            Some(l) => self.group_letter_take(&sender.did, l),
            None => self.log("a group letter does not decode"),
        }
        ctrl.send(&Msg::GroupLetterTaken).await?;
        ctrl.finish();
        Ok(())
    }

    fn group_letter_take(self: &Arc<Self>, from: &str, letter: Letter) {
        let Ok(core) = self.group_core() else { return };
        match letter {
            Letter::Invite { id, token, inviter_name } => {
                let id = hex::encode(id);
                if core.store.inv(&id).ok().flatten().is_some() {
                    return;
                }
                let Ok(t) = cryptography::codec::from_bytes::<InviteToken>(&token, MAX_TOKEN)
                else {
                    return;
                };
                let Some(inv) = t.invite.as_ref().and_then(|i| i.open().ok()) else {
                    return;
                };
                // The sender signed it, for this group, and it has not run out.
                if inv.issuer != from || inv.group != t.group || inv.expires < now() || !super::valid_gid(&t.group) || t.name.len() > MAX_NAME {
                    self.log(format!("a group invitation from {from} does not check out"));
                    return;
                }
                let inviter_name = crate::block::known_name(&self.shared.lock().state, from).unwrap_or_else(|| inviter_name.chars().take(100).collect());
                let invitee_name = self.me().map(|m| m.profile.name.clone()).unwrap_or_default();
                let row = InvRow {
                    id,
                    group: t.group.clone(),
                    group_name: t.name.clone(),
                    inviter: from.to_string(),
                    inviter_name,
                    invitee: core.did.clone(),
                    invitee_name,
                    outgoing: false,
                    state: GroupInvitationState::Pending,
                    at: now_ms(),
                    expires: inv.expires,
                    token,
                    delivered: true,
                    owe: false,
                };
                let row = self.inv_current(&core, row);
                self.log(format!("group invitation from {from} to {}", row.group_name));
                self.inv_save(&core, &row);
            }
            Letter::Answer { id, joined } => {
                let Some(mut r) = core.store.inv(&hex::encode(id)).ok().flatten() else { return };
                if !r.outgoing || r.invitee != from || final_state(r.state) {
                    return;
                }
                r.state = if joined { GroupInvitationState::Joined } else { GroupInvitationState::Declined };
                r.owe = false;
                r.delivered = true;
                self.inv_save(&core, &r);
            }
        }
    }

    // ---- own devices (self/3) -----------------------------------------------------------------

    /// Our invitation rows for our other devices (without the outbox flag).
    pub(crate) fn group_own_invitations(self: &Arc<Self>) -> Vec<Vec<u8>> {
        let Ok(core) = self.group_core() else { return vec![] };
        let t = now();
        let mut rows: Vec<InvRow> = core.store.invs().unwrap_or_default().into_iter().filter(|r| r.expires + KEEP_AFTER_EXPIRY >= t).collect();
        rows.sort_by(|a, b| b.at.cmp(&a.at));
        rows.truncate(MAX_OWN_INVITATIONS);
        let mut out: Vec<Vec<u8>> = rows
            .into_iter()
            .map(|mut r| {
                r.owe = false;
                json(&r)
            })
            .collect();
        out.sort();
        out
    }

    /// Rows from one of our other devices: new ones are added, an answer replaces Pending.
    pub(crate) fn group_own_invitations_apply(self: &Arc<Self>, rows: &[Vec<u8>]) {
        let Ok(core) = self.group_core() else { return };
        let Ok(me) = self.me() else { return };
        let did = me.id.did().to_string();
        for b in rows.iter().take(MAX_OWN_INVITATIONS) {
            let Ok(mut r) = serde_json::from_slice::<InvRow>(b) else { continue };
            // Only our own: we invited, or we were invited.
            if (r.outgoing && r.inviter != did) || (!r.outgoing && r.invitee != did) || id_bytes(&r.id).is_none() {
                continue;
            }
            r.owe = false;
            match core.store.inv(&r.id).ok().flatten() {
                None => {
                    // An incoming one must still carry an invite its inviter signed.
                    let ok = cryptography::codec::from_bytes::<InviteToken>(&r.token, MAX_TOKEN)
                        .ok()
                        .and_then(|t| {
                            t.invite
                                .as_ref()
                                .and_then(|i| i.open().ok())
                                .map(|i| (i, t.group))
                        })
                        .is_some_and(|(i, g)| {
                            i.issuer == r.inviter && i.group == g && g == r.group
                        });
                    if ok {
                        self.inv_save(&core, &r);
                    }
                }
                Some(mut cur) if !final_state(cur.state) && final_state(r.state) => {
                    cur.state = r.state;
                    cur.delivered |= r.delivered;
                    // A decline made on another device is delivered by that device.
                    if !cur.outgoing {
                        cur.owe = false;
                    }
                    self.inv_save(&core, &cur);
                }
                Some(mut cur) if r.delivered && !cur.delivered => {
                    cur.delivered = true;
                    let _ = core.store.put_inv(&cur);
                }
                Some(_) => {}
            }
        }
    }
}

mod node;
