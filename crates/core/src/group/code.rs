//! Short invite codes (G3): the runtime is `group_runtime::code`; these are the `Node` calls.

pub use group_runtime::code::*;

use std::time::{Duration, Instant};

use group_runtime::record::*;
use group_runtime::wire::*;
use tokio::sync::oneshot;

use crate::Error;
use crate::node::{Node, addr_for};

#[uniffi::export]
impl Node {
    /// Makes the host forget an invite link (`group_invite`'s text, or the bare code): nobody can
    /// fetch it any more. The issuer or an admin; an invite someone already fetched stays valid
    /// until it expires.
    pub fn group_revoke_invite(&self, link: String) -> Result<(), Error> {
        let sc = ShortCode::parse(&link).ok_or_else(|| Error::Protocol("not a group invite link".into()))?;
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group.group_core()?;
            if let Some(row) = core.store.code(&sc.code)? {
                let admin = core.groups.lock().get(&row.gid).is_some_and(|l| l.state.is_admin(&core.did));
                if row.by != core.did && !admin {
                    return Err(Error::Rejected("only its issuer or an admin can revoke an invite".into()));
                }
                return Ok(core.store.drop_code(&sc.code)?);
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
    /// The token a link's host hands out (`codec` bytes), without joining.
    pub fn group_fetch_token_for_test(&self, link: String) -> Option<Vec<u8>> {
        let sc = ShortCode::parse(&link)?;
        let inner = self.inner.clone();
        let t = self.block_on(async move { inner.group.group_fetch(&sc).await }).ok()?.ok()?;
        cryptography::codec::to_bytes(&t).ok()
    }

    /// Joins with a token fetched earlier (`group_fetch_token_for_test`): what a joiner holding a
    /// stale open link's token does after the group closed.
    pub fn group_join_token_for_test(&self, token: Vec<u8>) -> Result<String, Error> {
        let t: InviteToken = cryptography::codec::from_bytes(&token, MAX_TOKEN).map_err(|_| Error::Protocol("bad token".into()))?;
        let inner = self.inner.clone();
        Ok(self.block_on(async move { inner.group.group_join_token(t).await })??)
    }

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
            let Ok(core) = inner.group.group_core() else { return out };
            let Ok(ep) = inner.group.group_ensure(&core).await else { return out };
            let Ok(addr) = addr_for(&sc.endpoint, sc.relay.as_deref()) else { return out };
            let Ok(Ok(conn)) = tokio::time::timeout(FETCH_TIMEOUT, ep.connect(addr, GROUP_ALPN)).await else { return out };
            let Ok((send, recv)) = conn.open_bi().await else { return out };
            let (mut w, mut r) = (Writer(send), Reader::new(recv));
            for _ in 0..n {
                if w.send(&GroupMsg::Fetch { code: cryptography::rng::bytes() }).await.is_err() {
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
