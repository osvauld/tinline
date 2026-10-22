//! Short invite codes (G3). The shareable text no longer carries the invite: a host keeps the
//! full signed invite under a random code, and the link carries only what is needed to reach
//! that ONE host and ask for it:
//!
//! ```text
//! tinline://g/<base58( 0x01 ‖ host group endpoint (32) ‖ code (12) ‖ relay hint )>
//! relay hint: 0x00 none | 0x01 idx (a known n0 relay) | 0x02 len name (another *.relay.n0.iroh.link)
//!             | 0x03 len url (anything else)
//! ```
//!
//! With a known n0 relay that is 47 bytes, 64-65 base58 characters, 76-77 with the prefix. The
//! bare code (without `tinline://g/`) is accepted too.
//!
//! Joining (and peeking) dials the host on the group endpoint, sends `Fetch{code}` and gets
//! `Fetched{token}`: the signed invite, the group, its name and owner and the hosts' DID-signed
//! addresses. Everything is then verified exactly as before (the invite's DID signature, group and
//! expiry here; the issuer's role by the host at `Hello` and by every device from the records).
//! The fetch reply must name the dialled endpoint among the group's signed host addresses.
//!
//! Hosts keep codes until the invite expires (7 days), forget them then, refuse unknown or
//! expired codes with a clear reason, slow down wrong guesses and close a connection after
//! `MAX_BAD_PER_CONN` of them (and stop answering fetches for a minute after
//! `MAX_BAD_PER_MINUTE` wrong codes from anyone). A code is 96 random bits.
//!
//! An admin that does not host registers its code at a host over its member session (`Code`);
//! the link then names that host. Revoking (`group_revoke_invite`) makes the host forget the code;
//! an invite that someone already fetched stays valid until it expires (it is a bearer grant).
//! Like the old token, a link reveals one host's group endpoint to whoever holds it.
//!
//! The link format (`ShortCode`) and the code rows live in `group_core::code`, re-exported here.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::oneshot;

pub use group_core::code::*;

use super::engine::{GroupCore, INVITE_TTL};
use super::record::*;
use super::wire::*;
use crate::Error;
use crate::GroupRt;
use node_runtime::{addr_for, now, now_ms};

/// Unknown codes a host answers on one connection before closing it.
pub const MAX_BAD_PER_CONN: u32 = 3;
/// Unknown codes (from anyone) per minute before a host stops answering fetches for a while.
pub const MAX_BAD_PER_MINUTE: u32 = 30;
/// How long a host waits before answering an unknown code.
const BAD_DELAY: Duration = Duration::from_millis(300);
/// Codes one group may hold at a host (PROVISIONAL (coordinator)).
pub const MAX_CODES_PER_GROUP: usize = 256;
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
/// The whole of `group_fetch`, retries included.
const FETCH_DEADLINE: Duration = Duration::from_secs(40);
pub const ACK_TIMEOUT: Duration = Duration::from_secs(15);
/// How long `group_invite` waits for a session with a host when we do not host.
pub const HOST_WAIT: Duration = Duration::from_secs(30);

/// Runtime state (not persisted).
#[derive(Default)]
pub struct CodeRt {
    /// Answers to our `Code` / `RevokeCode`, by code.
    pub waiters: Mutex<HashMap<[u8; CODE_LEN], oneshot::Sender<(bool, String)>>>,
    /// Wrong codes in the current minute.
    bad: Mutex<(Option<Instant>, u32)>,
    /// When expired codes were last pruned (unix s).
    pub pruned: Mutex<u64>,
}

impl CodeRt {
    /// Counts one wrong code; true while fetches are still answered.
    fn bad_one(&self) -> bool {
        let mut b = self.bad.lock();
        let now = Instant::now();
        if b.0.is_none_or(|t| now.duration_since(t) > Duration::from_secs(60)) {
            *b = (Some(now), 0);
        }
        b.1 += 1;
        b.1 <= MAX_BAD_PER_MINUTE
    }

    fn throttled(&self) -> bool {
        let b = self.bad.lock();
        b.0.is_some_and(|t| t.elapsed() <= Duration::from_secs(60)) && b.1 > MAX_BAD_PER_MINUTE
    }
}

pub const UNKNOWN_CODE: &str = "unknown or expired invite code";
pub const NOT_OPEN: &str = "this group is not open to new members";
pub const OPEN_LINK_WITHDRAWN: &str = "this open link was withdrawn; ask for a new one";
/// `group_open_code`: how many `NOT_OPEN` answers (a host still catching up on our `Open`
/// record) are asked again, across all hosts, before the refusal stands. Each retry waits for
/// the next 500 ms round, and the whole call stays inside `HOST_WAIT`.
const NOT_OPEN_RETRIES: u32 = 10;

/// Checks a fetched token against the code it was fetched with: the dialled endpoint is one of
/// the group's signed host addresses, and the invite (if any) is signed, for this group, unexpired.
pub fn check_fetched(t: &InviteToken, sc: &ShortCode, me: &str) -> Result<(), Error> {
    let names_host = t.hosts.iter().any(|h| {
        h.open()
            .ok()
            .is_some_and(|a| a.group == t.group && a.endpoint == sc.endpoint)
    });
    if !names_host || !super::valid_gid(&t.group) || t.name.len() > MAX_NAME {
        return Err(Error::Protocol("the host's answer does not match the invite link".into()));
    }
    if let Some(inv) = &t.invite
        && redeem_invite(inv, &t.group, me, now_ms(), &proto::Unrecorded).is_none()
    {
        return Err(Error::Rejected("invite expired or invalid".into()));
    }
    Ok(())
}

impl GroupRt {
    /// Makes a code for `invite` (`None`: a device code) and has a host keep it: ourselves if we
    /// host `gid`, else a host we have a session with. Returns the link.
    pub async fn group_issue_code(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, invite: Option<SignedInvite>) -> Result<String, Error> {
        let code: [u8; CODE_LEN] = cryptography::rng::bytes();
        let kind = if invite.is_some() { CodeKind::Invite } else { CodeKind::Device };
        let expires = invite
            .as_ref()
            .and_then(|i| i.open().ok())
            .map_or(now_ms() + INVITE_TTL, |i| i.exp_ms);
        // Only a CHAT host keeps codes (`group_fetch_answer` checks `is_host`); `group_my_addr`
        // is also Some for a device that only hosts calls (GA-I1), which registers at a host.
        let chat_host = core.am_host(gid);
        if chat_host
            && let Some(mine) = self.group_my_addr(core, gid)
            && let Some(a) = mine.open().ok()
        {
            if core.store.prune_codes(Some(gid))? >= MAX_CODES_PER_GROUP {
                return Err(Error::Rejected("too many open invites for this group; revoke some".into()));
            }
            let row = CodeRow { gid: gid.to_string(), kind, invite: invite.as_ref().map(|i| cryptography::codec::to_bytes(i).expect("serialises")), expires, by: core.did.clone() };
            core.store.put_code(&code, &row)?;
            core.store.put_issued(&code, &IssuedRow { gid: gid.to_string(), host_did: core.did.clone(), host_dev: hex::encode(core.dev), expires })?;
            return Ok(ShortCode { endpoint: a.endpoint, relay: a.relay, code }.to_link());
        }
        // Not a host: register it at a host we are connected to (waiting a while for a session).
        let end = Instant::now() + HOST_WAIT;
        let (host_did, host_dev) = loop {
            let (tx, rx) = oneshot::channel();
            core.codes.waiters.lock().insert(code, tx);
            if let Some(h) = core.send_to_a_host(gid, GroupMsg::Code { code, kind, invite: invite.clone(), expires }) {
                match tokio::time::timeout(ACK_TIMEOUT, rx).await {
                    Ok(Ok((true, _))) => break h,
                    Ok(Ok((false, reason))) => return Err(Error::Rejected(reason)),
                    _ => {}
                }
            }
            core.codes.waiters.lock().remove(&code);
            if Instant::now() > end {
                return Err(Error::Rejected("no host of this group is reachable right now".into()));
            }
            core.kick_pub();
            tokio::time::sleep(Duration::from_millis(500)).await;
        };
        let addr = core
            .store
            .hosts(gid)?
            .into_iter()
            .filter_map(|s| s.open().ok())
            .find(|a| a.did == host_did && a.dev == host_dev)
            .ok_or_else(|| Error::Rejected("the host's address is unknown".into()))?;
        core.store.put_issued(&code, &IssuedRow { gid: gid.to_string(), host_did, host_dev: hex::encode(host_dev), expires })?;
        Ok(ShortCode { endpoint: addr.endpoint, relay: addr.relay, code }.to_link())
    }

    /// An open link for `gid` (public group §2): one code with no invite and no expiry, kept by
    /// every chat host reachable now (ourselves if we host, then each host we have a session
    /// with), so closing can drop it everywhere. The link names us if we host, else the first
    /// host that took it. The caller has written `Open{active: true}` before: a host refuses the
    /// code unless the group is open (that record goes first on the same session stream).
    ///
    /// `reuse`: register this code (our stored link's) again instead of a new one, so a host that
    /// lost it (or never had it) holds it again. Each host keeps one open code per issuer: ours
    /// replaces the one we registered before.
    pub async fn group_open_code(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, reuse: Option<[u8; CODE_LEN]>) -> Result<String, Error> {
        let code: [u8; CODE_LEN] = reuse.unwrap_or_else(cryptography::rng::bytes);
        let mut link = None;
        if core.am_host(gid)
            && let Some(mine) = self.group_my_addr(core, gid)
            && let Some(a) = mine.open().ok()
        {
            core.store.drop_open_codes(gid, Some(&core.did), Some(&code))?;
            if core.store.code(&code)?.is_none() && core.store.prune_codes(Some(gid))? >= MAX_CODES_PER_GROUP {
                return Err(Error::Rejected("too many open invites for this group; revoke some".into()));
            }
            core.store.put_code(&code, &CodeRow { gid: gid.to_string(), kind: CodeKind::Open, invite: None, expires: NO_EXPIRY, by: core.did.clone() })?;
            core.store.put_issued(&code, &IssuedRow { gid: gid.to_string(), host_did: core.did.clone(), host_dev: hex::encode(core.dev), expires: NO_EXPIRY })?;
            link = Some(ShortCode { endpoint: a.endpoint, relay: a.relay, code }.to_link());
        }
        let end = Instant::now() + HOST_WAIT;
        let mut tried: std::collections::HashSet<(String, [u8; 32])> = Default::default();
        let mut refused = None;
        let mut not_open_retries = 0;
        loop {
            let hosts: Vec<(String, [u8; 32])> = core.with_state(gid, |st| st.hosts.iter().map(|h| (h.did.clone(), h.dev)).collect()).unwrap_or_default();
            for (did, dev) in hosts {
                if (did == core.did && dev == core.dev) || tried.contains(&(did.clone(), dev)) || !core.has_session(gid, &did, &dev) {
                    continue;
                }
                tried.insert((did.clone(), dev));
                let (tx, rx) = oneshot::channel();
                core.codes.waiters.lock().insert(code, tx);
                let msg = GroupMsg::Code { code, kind: CodeKind::Open, invite: None, expires: NO_EXPIRY };
                let answer = if core.send_to_host(gid, &did, &dev, msg) { tokio::time::timeout(ACK_TIMEOUT, rx).await.ok().and_then(|r| r.ok()) } else { None };
                core.codes.waiters.lock().remove(&code);
                match answer {
                    Some((true, _)) if link.is_none() => {
                        let addr = core.store.hosts(gid)?.into_iter().filter_map(|s| s.open().ok()).find(|a| a.did == did && a.dev == dev);
                        if let Some(a) = addr {
                            core.store.put_issued(&code, &IssuedRow { gid: gid.to_string(), host_did: did.clone(), host_dev: hex::encode(dev), expires: NO_EXPIRY })?;
                            link = Some(ShortCode { endpoint: a.endpoint, relay: a.relay, code }.to_link());
                        }
                    }
                    Some((true, _)) => {}
                    // The host has not applied our `Open` record yet (its catch-up is still in
                    // flight): ask it again on a later round, a bounded number of times.
                    Some((false, reason)) if reason == NOT_OPEN && not_open_retries < NOT_OPEN_RETRIES => {
                        not_open_retries += 1;
                        tried.remove(&(did, dev));
                    }
                    Some((false, reason)) => refused = Some(reason),
                    None => {
                        tried.remove(&(did, dev));
                    }
                }
            }
            if let Some(l) = link {
                return Ok(l);
            }
            if let Some(reason) = refused.take() {
                return Err(Error::Rejected(reason));
            }
            if Instant::now() > end {
                return Err(Error::Rejected("no host of this group is reachable right now".into()));
            }
            core.kick_pub();
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Closing an open group: forget its open link here and have every host we reach forget it
    /// (best effort; a host we miss drops it at its next fetch, as the group is closed).
    pub fn group_drop_open_code(&self, core: &GroupCore, gid: &str, link: &str) {
        let Some(sc) = ShortCode::parse(link) else { return };
        let _ = core.store.drop_code(&sc.code);
        let hosts: Vec<(String, [u8; 32])> = core.with_state(gid, |st| st.hosts.iter().map(|h| (h.did.clone(), h.dev)).collect()).unwrap_or_default();
        for (did, dev) in hosts {
            core.send_to_host(gid, &did, &dev, GroupMsg::RevokeCode { code: sc.code });
        }
    }

    /// After a meta change: while `gid` is closed (derived `open` now), this device keeps no
    /// open code of it (as a host: every issuer's, so another admin's close, or a close we missed
    /// the revoke of, leaves nothing a later reopen would revive) and forgets its own stored open
    /// link (an admin gets a new one after reopening).
    pub fn group_open_codes_closed(&self, core: &GroupCore, gid: &str) {
        let closed = {
            let mut groups = core.groups.lock();
            let Some(live) = groups.get_mut(gid) else { return };
            if live.state.open || live.state.owner.is_none() {
                return;
            }
            if let Some(link) = live.meta.open_link.take() {
                live.meta.open_link_record = None;
                let _ = core.store.put_meta(gid, &live.meta);
                // Our issued row of it (a code we registered at another host) goes too.
                if let Some(sc) = ShortCode::parse(&link) {
                    let _ = core.store.drop_code(&sc.code);
                }
            }
            true
        };
        if closed && let Err(e) = core.store.drop_open_codes(gid, None, None) {
            tracing::debug!("group {}: dropping open codes: {e}", super::short(gid));
        }
    }

    /// Asks the host a link names for the invite behind it. GL: a lost connection or a timeout
    /// is tried again (pauses of 0.5, 1, 2, 4 s) until `FETCH_DEADLINE`, the worst case of a single
    /// attempt before (connect + answer); a refusal or anything else comes back at once.
    pub async fn group_fetch(self: &Arc<Self>, sc: &ShortCode) -> Result<InviteToken, Error> {
        let end = Instant::now() + FETCH_DEADLINE;
        let mut pause = Duration::from_millis(500);
        loop {
            let step = end.saturating_duration_since(Instant::now()).min(FETCH_TIMEOUT);
            match self.group_fetch_once(sc, step).await {
                Err(e @ (Error::Timeout | Error::Net(_))) => {
                    if Instant::now() + pause + Duration::from_secs(1) >= end {
                        return Err(e);
                    }
                    tracing::info!("fetching an invite: {e}; trying again");
                    tokio::time::sleep(pause).await;
                    pause = (pause * 2).min(Duration::from_secs(4));
                }
                r => return r,
            }
        }
    }

    /// One attempt of `group_fetch`, each network step bounded by `step`.
    async fn group_fetch_once(self: &Arc<Self>, sc: &ShortCode, step: Duration) -> Result<InviteToken, Error> {
        let core = self.group_core()?;
        let ep = self.group_ensure(&core).await?;
        if ep.id().as_bytes() == &sc.endpoint {
            // Our own link: answer from our own store.
            return self.group_fetch_answer(&core, &sc.code).map_err(Error::Rejected);
        }
        if super::engine::injected(&self.fail_fetches) {
            return Err(Error::Net("connection lost (injected)".into()));
        }
        let addr = addr_for(&sc.endpoint, sc.relay.as_deref())?;
        let conn = tokio::time::timeout(step, ep.connect(addr, GROUP_ALPN)).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut w = Writer(send);
        let mut r = Reader::new(recv);
        w.send(&GroupMsg::Fetch { code: sc.code }).await?;
        let reply = tokio::time::timeout(step, r.recv()).await.map_err(|_| Error::Timeout)??;
        let _ = w.0.finish();
        conn.close(0u32.into(), b"bye");
        match reply {
            Some(GroupMsg::Fetched { token }) => {
                check_fetched(&token, sc, &core.did)?;
                Ok(token)
            }
            Some(GroupMsg::Refused { reason }) => Err(Error::Rejected(reason.chars().take(200).collect())),
            // The host's side went away before answering: as transient as a lost connection.
            None => Err(Error::Net("connection closed before an answer".into())),
            _ => Err(Error::Protocol("unexpected answer to an invite code".into())),
        }
    }

    /// Host side: the token for `code`, or why not.
    fn group_fetch_answer(self: &Arc<Self>, core: &Arc<GroupCore>, code: &[u8; CODE_LEN]) -> Result<InviteToken, String> {
        let row = core.store.code(code).ok().flatten().ok_or_else(|| UNKNOWN_CODE.to_string())?;
        let (hosting, name, owner, open, issuer_admin) = {
            let groups = core.groups.lock();
            let Some(l) = groups.get(&row.gid) else { return Err(UNKNOWN_CODE.into()) };
            (l.state.is_host(&core.did, &core.dev), l.state.name.clone(), l.state.owner.clone().unwrap_or_default(), l.state.open, l.state.is_admin(&row.by))
        };
        if !hosting {
            return Err("this device no longer hosts that group; ask for a new invite".into());
        }
        let open_link = row.kind == CodeKind::Open;
        if open_link && !open {
            // Closed since (a close we missed the revoke of): the link is dead for good.
            let _ = core.store.drop_code(code);
            return Err(NOT_OPEN.into());
        }
        if open_link && !issuer_admin {
            // Its issuer is no longer an admin (or no longer a member): the link goes with the role.
            let _ = core.store.drop_code(code);
            return Err(OPEN_LINK_WITHDRAWN.into());
        }
        let _ = self.group_my_addr(core, &row.gid);
        let invite = match &row.invite {
            Some(b) => Some(cryptography::codec::from_bytes::<SignedInvite>(b, node_runtime::store::MAX_ROW).map_err(|_| UNKNOWN_CODE.to_string())?),
            None => None,
        };
        Ok(InviteToken { group: row.gid.clone(), name, owner, invite, hosts: core.chat_host_addrs(&row.gid), open: open_link })
    }

    /// Host side: a connection that opened with `Fetch`. Answers up to `MAX_BAD_PER_CONN` unknown
    /// codes (each after a short delay), then closes.
    pub async fn group_fetch_session(self: &Arc<Self>, core: &Arc<GroupCore>, mut writer: Writer, mut reader: Reader, first: [u8; CODE_LEN]) {
        let mut code = first;
        let mut bad = 0u32;
        loop {
            if core.codes.throttled() {
                let _ = writer.send(&GroupMsg::Refused { reason: "too many wrong invite codes; try again later".into() }).await;
                break;
            }
            match self.group_fetch_answer(core, &code) {
                Ok(token) => {
                    if writer.send(&GroupMsg::Fetched { token }).await.is_err() {
                        break;
                    }
                }
                Err(reason) => {
                    bad += 1;
                    core.codes.bad_one();
                    tokio::time::sleep(BAD_DELAY).await;
                    if writer.send(&GroupMsg::Refused { reason }).await.is_err() || bad >= MAX_BAD_PER_CONN {
                        break;
                    }
                }
            }
            match tokio::time::timeout(FETCH_TIMEOUT, reader.recv()).await {
                Ok(Ok(Some(GroupMsg::Fetch { code: c }))) => code = c,
                _ => break,
            }
        }
        let _ = writer.0.finish();
    }

    /// Host side, inside a member's session: `Code` / `RevokeCode`.
    pub fn group_code_in(&self, core: &GroupCore, gid: &str, did: &str, msg: GroupMsg) -> GroupMsg {
        let (member, admin, open) = core.groups.lock().get(gid).map(|l| (l.state.is_member(did) && !l.state.is_removed(did), l.state.is_admin(did), l.state.open)).unwrap_or_default();
        let ack = |code, ok: bool, reason: &str| GroupMsg::CodeAck { code, ok, reason: reason.to_string() };
        match msg {
            GroupMsg::Code { code, kind, invite, expires } => {
                if !member {
                    return ack(code, false, "not a member");
                }
                let expires = match (kind, &invite) {
                    (CodeKind::Invite, Some(i)) => match redeem_invite(i, gid, did, now_ms(), &core.store) {
                        Some(inv) if inv.iss == did && admin => inv.exp_ms,
                        _ => return ack(code, false, "only admins can invite"),
                    },
                    (CodeKind::Device, None) => expires.min(now_ms() + INVITE_TTL),
                    // The admin's `Open{active: true}` went before this frame on the same stream.
                    (CodeKind::Open, None) if !admin => return ack(code, false, "only admins can share an open link"),
                    (CodeKind::Open, None) if !open => return ack(code, false, NOT_OPEN),
                    (CodeKind::Open, None) => NO_EXPIRY,
                    _ => return ack(code, false, "malformed invite code"),
                };
                if expires <= now_ms() {
                    return ack(code, false, "invite expired");
                }
                let held = core.store.code(&code).ok().flatten();
                // An admin registering its open link again (a host that lost it, `group_open_link`)
                // is fine; any other existing code is someone else's.
                let again = held.as_ref().is_some_and(|r| r.gid == gid && r.kind == CodeKind::Open && kind == CodeKind::Open && r.by == did);
                if held.is_some() && !again {
                    return ack(code, false, "code in use");
                }
                // One open link per issuer per group: this one replaces the issuer's earlier one.
                if kind == CodeKind::Open && core.store.drop_open_codes(gid, Some(did), Some(&code)).is_err() {
                    return ack(code, false, "could not store the code");
                }
                match core.store.prune_codes(Some(gid)) {
                    Ok(_) if again => {}
                    Ok(n) if n < MAX_CODES_PER_GROUP => {}
                    _ => return ack(code, false, "too many open invites for this group; revoke some"),
                }
                let row = CodeRow { gid: gid.to_string(), kind, invite: invite.as_ref().map(|i| cryptography::codec::to_bytes(i).expect("serialises")), expires, by: did.to_string() };
                match core.store.put_code(&code, &row) {
                    Ok(()) => ack(code, true, ""),
                    Err(e) => ack(code, false, &e.to_string()),
                }
            }
            GroupMsg::RevokeCode { code } => match core.store.code(&code).ok().flatten() {
                Some(r) if r.gid == gid && (r.by == did || admin) => {
                    let _ = core.store.drop_code(&code);
                    ack(code, true, "")
                }
                Some(_) => ack(code, false, "only its issuer or an admin can revoke an invite"),
                None => ack(code, true, ""),
            },
            _ => ack([0; CODE_LEN], false, "unexpected"),
        }
    }

    /// Member side: a host answered our `Code` / `RevokeCode`.
    pub fn group_code_ack_in(&self, core: &GroupCore, code: [u8; CODE_LEN], ok: bool, reason: String) {
        if let Some(tx) = core.codes.waiters.lock().remove(&code) {
            let _ = tx.send((ok, reason));
        }
    }

    /// Prunes expired codes now and then (called from the dial loop).
    pub fn group_codes_tick(&self, core: &GroupCore) {
        let t = now();
        {
            let mut last = core.codes.pruned.lock();
            if t < *last + 300 {
                return;
            }
            *last = t;
        }
        if let Err(e) = core.store.prune_codes(None) {
            tracing::debug!("pruning invite codes: {e}");
        }
    }
}
