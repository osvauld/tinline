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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use storage::Op;
use tokio::sync::oneshot;

use super::engine::{GroupCore, INVITE_TTL};
use super::record::*;
use super::store::{GroupStore, io, json};
use super::wire::*;
use crate::Error;
use crate::node::{Inner, Node, addr_for, now};

pub const LINK_PREFIX: &str = "tinline://g/";
pub const CODE_LEN: usize = 12;
const VERSION: u8 = 1;
const N0_SUFFIX: &str = ".relay.n0.iroh.link";
/// The n0 relays (iroh 1.3 `presets::N0`), by index.
const N0_RELAYS: [&str; 4] = ["use1-1", "usw1-1", "euc1-1", "aps1-1"];

/// Unknown codes a host answers on one connection before closing it.
pub const MAX_BAD_PER_CONN: u32 = 3;
/// Unknown codes (from anyone) per minute before a host stops answering fetches for a while.
pub const MAX_BAD_PER_MINUTE: u32 = 30;
/// How long a host waits before answering an unknown code.
const BAD_DELAY: Duration = Duration::from_millis(300);
/// Codes one group may hold at a host (PROVISIONAL (coordinator)).
pub const MAX_CODES_PER_GROUP: usize = 256;
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const ACK_TIMEOUT: Duration = Duration::from_secs(15);
/// How long `group_invite` waits for a session with a host when we do not host.
const HOST_WAIT: Duration = Duration::from_secs(30);

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
        let body = if t.len() >= LINK_PREFIX.len() && t[..LINK_PREFIX.len()].eq_ignore_ascii_case(LINK_PREFIX) { &t[LINK_PREFIX.len()..] } else { t };
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

/// Host side: an invite (or a device token) kept under a code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodeRow {
    pub gid: String,
    /// bincode `SignedInvite`; `None` for a device code.
    pub invite: Option<Vec<u8>>,
    /// Unix seconds.
    pub expires: u64,
    /// Who registered it.
    pub by: String,
}

/// Issuer side: which group and host a code we made is at (for revoking it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IssuedRow {
    pub gid: String,
    pub host_did: String,
    pub host_dev: String,
    pub expires: u64,
}

/// Runtime state (not persisted).
#[derive(Default)]
pub(crate) struct CodeRt {
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

pub(crate) const UNKNOWN_CODE: &str = "unknown or expired invite code";

impl GroupStore {
    pub(crate) fn put_code(&self, code: &[u8; CODE_LEN], r: &CodeRow) -> Result<(), Error> {
        self.db.put(&format!("code/{}", hex::encode(code)), &json(r)).map_err(io)
    }

    /// The code's row if it is unexpired (an expired one is forgotten here).
    pub(crate) fn code(&self, code: &[u8; CODE_LEN]) -> Result<Option<CodeRow>, Error> {
        let k = format!("code/{}", hex::encode(code));
        let Some(r) = self.db.get(&k).map_err(io)?.and_then(|v| serde_json::from_slice::<CodeRow>(&v).ok()) else { return Ok(None) };
        if r.expires < now() {
            self.db.apply(&[Op::Delete(k)]).map_err(io)?;
            return Ok(None);
        }
        Ok(Some(r))
    }

    pub(crate) fn drop_code(&self, code: &[u8; CODE_LEN]) -> Result<(), Error> {
        self.db.apply(&[Op::Delete(format!("code/{}", hex::encode(code))), Op::Delete(format!("issued/{}", hex::encode(code)))]).map_err(io)
    }

    /// Forgets every expired code (and issued row); returns how many codes `gid` still holds.
    pub(crate) fn prune_codes(&self, gid: Option<&str>) -> Result<usize, Error> {
        let t = now();
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

    pub(crate) fn put_issued(&self, code: &[u8; CODE_LEN], r: &IssuedRow) -> Result<(), Error> {
        self.db.put(&format!("issued/{}", hex::encode(code)), &json(r)).map_err(io)
    }

    pub(crate) fn issued(&self, code: &[u8; CODE_LEN]) -> Result<Option<IssuedRow>, Error> {
        Ok(self.db.get(&format!("issued/{}", hex::encode(code))).map_err(io)?.and_then(|v| serde_json::from_slice(&v).ok()))
    }
}

/// Checks a fetched token against the code it was fetched with: the dialled endpoint is one of
/// the group's signed host addresses, and the invite (if any) is signed, for this group, unexpired.
pub(crate) fn check_fetched(t: &InviteToken, sc: &ShortCode) -> Result<(), Error> {
    let names_host = t.hosts.iter().any(|h| h.open().is_some_and(|a| a.group == t.group && a.endpoint == sc.endpoint));
    if !names_host || t.group.len() != 64 || t.name.len() > MAX_NAME {
        return Err(Error::Protocol("the host's answer does not match the invite link".into()));
    }
    if let Some(inv) = &t.invite
        && inv.open().is_none_or(|i| i.group != t.group || i.expires < now())
    {
        return Err(Error::Rejected("invite expired or invalid".into()));
    }
    Ok(())
}

impl Inner {
    /// Makes a code for `invite` (`None`: a device code) and has a host keep it: ourselves if we
    /// host `gid`, else a host we have a session with. Returns the link.
    pub(crate) async fn group_issue_code(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, invite: Option<SignedInvite>) -> Result<String, Error> {
        let code: [u8; CODE_LEN] = rand::random();
        let expires = invite.as_ref().and_then(|i| i.open()).map_or(now() + INVITE_TTL, |i| i.expires);
        // Only a CHAT host keeps codes (`group_fetch_answer` checks `is_host`); `group_my_addr`
        // is also Some for a device that only hosts calls (GA-I1), which registers at a host.
        let chat_host = core.groups.lock().get(gid).is_some_and(|l| l.state.is_host(&core.did, &core.dev));
        if chat_host
            && let Some(mine) = self.group_my_addr(core, gid)
            && let Some(a) = mine.open()
        {
            if core.store.prune_codes(Some(gid))? >= MAX_CODES_PER_GROUP {
                return Err(Error::Rejected("too many open invites for this group; revoke some".into()));
            }
            let row = CodeRow { gid: gid.to_string(), invite: invite.as_ref().map(|i| bincode::serialize(i).expect("serialises")), expires, by: core.did.clone() };
            core.store.put_code(&code, &row)?;
            core.store.put_issued(&code, &IssuedRow { gid: gid.to_string(), host_did: core.did.clone(), host_dev: hex::encode(core.dev), expires })?;
            return Ok(ShortCode { endpoint: a.endpoint, relay: a.relay, code }.to_link());
        }
        // Not a host: register it at a host we are connected to (waiting a while for a session).
        let end = Instant::now() + HOST_WAIT;
        let (host_did, host_dev) = loop {
            let (tx, rx) = oneshot::channel();
            core.codes.waiters.lock().insert(code, tx);
            if let Some(h) = core.send_to_a_host(gid, GroupMsg::Code { code, invite: invite.clone(), expires }) {
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
            .filter_map(|s| s.open())
            .find(|a| a.did == host_did && a.dev == host_dev)
            .ok_or_else(|| Error::Rejected("the host's address is unknown".into()))?;
        core.store.put_issued(&code, &IssuedRow { gid: gid.to_string(), host_did, host_dev: hex::encode(host_dev), expires })?;
        Ok(ShortCode { endpoint: addr.endpoint, relay: addr.relay, code }.to_link())
    }

    /// Asks the host a link names for the invite behind it.
    pub(crate) async fn group_fetch(self: &Arc<Self>, sc: &ShortCode) -> Result<InviteToken, Error> {
        let core = self.group_core()?;
        let ep = self.group_ensure(&core).await?;
        if ep.id().as_bytes() == &sc.endpoint {
            // Our own link: answer from our own store.
            return self.group_fetch_answer(&core, &sc.code).map_err(Error::Rejected);
        }
        let addr = addr_for(&sc.endpoint, sc.relay.as_deref())?;
        let conn = tokio::time::timeout(FETCH_TIMEOUT, ep.connect(addr, GROUP_ALPN)).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
        let mut w = Writer(send);
        let mut r = Reader::new(recv);
        w.send(&GroupMsg::Fetch { code: sc.code }).await?;
        let reply = tokio::time::timeout(FETCH_TIMEOUT, r.recv()).await.map_err(|_| Error::Timeout)??;
        let _ = w.0.finish();
        conn.close(0u32.into(), b"bye");
        match reply {
            Some(GroupMsg::Fetched { token }) => {
                check_fetched(&token, sc)?;
                Ok(token)
            }
            Some(GroupMsg::Refused { reason }) => Err(Error::Rejected(reason.chars().take(200).collect())),
            _ => Err(Error::Protocol("unexpected answer to an invite code".into())),
        }
    }

    /// Host side: the token for `code`, or why not.
    fn group_fetch_answer(self: &Arc<Self>, core: &Arc<GroupCore>, code: &[u8; CODE_LEN]) -> Result<InviteToken, String> {
        let row = core.store.code(code).ok().flatten().ok_or_else(|| UNKNOWN_CODE.to_string())?;
        let (hosting, name, owner) = {
            let groups = core.groups.lock();
            let Some(l) = groups.get(&row.gid) else { return Err(UNKNOWN_CODE.into()) };
            (l.state.is_host(&core.did, &core.dev), l.state.name.clone(), l.state.owner.clone().unwrap_or_default())
        };
        if !hosting {
            return Err("this device no longer hosts that group; ask for a new invite".into());
        }
        let _ = self.group_my_addr(core, &row.gid);
        let invite = match &row.invite {
            Some(b) => Some(bincode::deserialize::<SignedInvite>(b).map_err(|_| UNKNOWN_CODE.to_string())?),
            None => None,
        };
        Ok(InviteToken { group: row.gid.clone(), name, owner, invite, hosts: core.host_addrs_pub(&row.gid) })
    }

    /// Host side: a connection that opened with `Fetch`. Answers up to `MAX_BAD_PER_CONN` unknown
    /// codes (each after a short delay), then closes.
    pub(crate) async fn group_fetch_session(self: &Arc<Self>, core: &Arc<GroupCore>, mut writer: Writer, mut reader: Reader, first: [u8; CODE_LEN]) {
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
    pub(crate) fn group_code_in(&self, core: &GroupCore, gid: &str, did: &str, msg: GroupMsg) -> GroupMsg {
        let (member, admin) = core.groups.lock().get(gid).map(|l| (l.state.is_member(did) && !l.state.is_removed(did), l.state.is_admin(did))).unwrap_or_default();
        let ack = |code, ok: bool, reason: &str| GroupMsg::CodeAck { code, ok, reason: reason.to_string() };
        match msg {
            GroupMsg::Code { code, invite, expires } => {
                if !member {
                    return ack(code, false, "not a member");
                }
                let expires = match &invite {
                    Some(i) => match i.open() {
                        Some(inv) if inv.group == gid && inv.issuer == did && admin && inv.expires >= now() => inv.expires,
                        _ => return ack(code, false, "only admins can invite"),
                    },
                    None => expires.min(now() + INVITE_TTL),
                };
                if expires < now() {
                    return ack(code, false, "invite expired");
                }
                match core.store.prune_codes(Some(gid)) {
                    Ok(n) if n < MAX_CODES_PER_GROUP => {}
                    _ => return ack(code, false, "too many open invites for this group; revoke some"),
                }
                if core.store.code(&code).ok().flatten().is_some() {
                    return ack(code, false, "code in use");
                }
                let row = CodeRow { gid: gid.to_string(), invite: invite.as_ref().map(|i| bincode::serialize(i).expect("serialises")), expires, by: did.to_string() };
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
    pub(crate) fn group_code_ack_in(&self, core: &GroupCore, code: [u8; CODE_LEN], ok: bool, reason: String) {
        if let Some(tx) = core.codes.waiters.lock().remove(&code) {
            let _ = tx.send((ok, reason));
        }
    }

    /// Prunes expired codes now and then (called from the dial loop).
    pub(crate) fn group_codes_tick(&self, core: &GroupCore) {
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

#[uniffi::export]
impl Node {
    /// Makes the host forget an invite link (`group_invite`'s text, or the bare code): nobody can
    /// fetch it any more. The issuer or an admin; an invite someone already fetched stays valid
    /// until it expires.
    pub fn group_revoke_invite(&self, link: String) -> Result<(), Error> {
        let sc = ShortCode::parse(&link).ok_or_else(|| Error::Protocol("not a group invite link".into()))?;
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            if let Some(row) = core.store.code(&sc.code)? {
                let admin = core.groups.lock().get(&row.gid).is_some_and(|l| l.state.is_admin(&core.did));
                if row.by != core.did && !admin {
                    return Err(Error::Rejected("only its issuer or an admin can revoke an invite".into()));
                }
                return core.store.drop_code(&sc.code);
            }
            let Some(issued) = core.store.issued(&sc.code)? else { return Err(Error::NotFound) };
            let dev: [u8; 32] = hex::decode(&issued.host_dev).ok().and_then(|b| b.try_into().ok()).ok_or(Error::NotFound)?;
            let end = Instant::now() + HOST_WAIT;
            loop {
                let (tx, rx) = oneshot::channel();
                core.codes.waiters.lock().insert(sc.code, tx);
                if core.send_to_host(&issued.gid, &issued.host_did, &dev, GroupMsg::RevokeCode { code: sc.code })
                    && let Ok(Ok((ok, reason))) = tokio::time::timeout(ACK_TIMEOUT, rx).await
                {
                    if !ok {
                        return Err(Error::Rejected(reason));
                    }
                    let _ = core.store.drop_code(&sc.code);
                    return Ok(());
                }
                core.codes.waiters.lock().remove(&sc.code);
                if Instant::now() > end {
                    return Err(Error::Rejected("the host that keeps this invite is not reachable right now".into()));
                }
                core.kick_pub();
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        })?
    }
}

/// Test hooks (not for apps).
#[uniffi::export]
impl Node {
    /// The group endpoint id of the host `link` names.
    pub fn group_link_endpoint_for_test(&self, link: String) -> Option<String> {
        let sc = ShortCode::parse(&link)?;
        iroh::PublicKey::from_bytes(&sc.endpoint).ok().map(|k| k.to_string())
    }

    /// Sends `n` random codes to the host `link` names over ONE connection; the host's answers
    /// (refusal reasons) until it closes.
    pub fn group_guess_codes_for_test(&self, link: String, n: u32) -> Vec<String> {
        let Some(sc) = ShortCode::parse(&link) else { return vec![] };
        let inner = self.inner.clone();
        self.block_on(async move {
            let mut out = Vec::new();
            let Ok(core) = inner.group_core() else { return out };
            let Ok(ep) = inner.group_ensure(&core).await else { return out };
            let Ok(addr) = addr_for(&sc.endpoint, sc.relay.as_deref()) else { return out };
            let Ok(Ok(conn)) = tokio::time::timeout(FETCH_TIMEOUT, ep.connect(addr, GROUP_ALPN)).await else { return out };
            let Ok((send, recv)) = conn.open_bi().await else { return out };
            let (mut w, mut r) = (Writer(send), Reader::new(recv));
            for _ in 0..n {
                if w.send(&GroupMsg::Fetch { code: rand::random() }).await.is_err() {
                    break;
                }
                match tokio::time::timeout(FETCH_TIMEOUT, r.recv()).await {
                    Ok(Ok(Some(GroupMsg::Refused { reason }))) => out.push(reason),
                    Ok(Ok(Some(_))) => out.push("answered".into()),
                    _ => break,
                }
            }
            conn.close(0u32.into(), b"bye");
            out
        })
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // KIND: unit (G3 stage 2) — the link is short and round-trips, for every kind of relay hint.
    #[test]
    fn links_are_short_and_round_trip() {
        let cases = [
            Some("https://use1-1.relay.n0.iroh.link./".to_string()),
            Some("https://aps1-1.relay.n0.iroh.link/".to_string()),
            Some("https://xyz9-2.relay.n0.iroh.link./".to_string()),
            Some("http://127.0.0.1:3340/".to_string()),
            None,
        ];
        for relay in cases {
            let sc = ShortCode { endpoint: [0xab; 32], relay: relay.clone(), code: [0xff; CODE_LEN] };
            let link = sc.to_link();
            let back = ShortCode::parse(&link).unwrap();
            assert_eq!(back.endpoint, sc.endpoint);
            assert_eq!(back.code, sc.code);
            assert_eq!(back.relay.as_deref().and_then(relay_host), relay.as_deref().and_then(relay_host), "{link}");
            assert_eq!(back.relay.is_some(), relay.is_some());
            if relay.as_deref().is_some_and(|r| r.contains("use1-1") || r.contains("aps1-1")) || relay.is_none() {
                assert!(link.len() <= 80, "{} chars: {link}", link.len());
            }
            // The bare code and an upper-case scheme work too.
            assert_eq!(ShortCode::parse(link.strip_prefix(LINK_PREFIX).unwrap()).unwrap(), back);
            assert_eq!(ShortCode::parse(&link.replace("tinline://g/", "TINLINE://G/")).unwrap(), back);
        }
        let max = ShortCode { endpoint: [0xff; 32], relay: Some("https://euc1-1.relay.n0.iroh.link./".into()), code: [0xff; CODE_LEN] }.to_link();
        assert!(max.len() <= 80, "{}", max.len());
        assert!(ShortCode::parse("tlgroup1:AAAA").is_none());
        assert!(ShortCode::parse("tinline://g/").is_none());
        assert!(ShortCode::parse("tinline://g/0OIl").is_none());
    }
}
