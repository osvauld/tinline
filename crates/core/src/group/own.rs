//! Own-device sync of the group list (G2 stage 2). Our linked devices (same DID) tell each other
//! which groups the account is in, over the existing `tinline/self/3` session
//! (`SelfMsg::Groups`): per group the same thing a `group_device_token` carries (group id, name,
//! owner, the active hosts' DID-signed addresses; no invite). A device that lacks a group adds it
//! and dials its hosts like a sibling holding a device token would; the hosts admit it because
//! the DID is a member. Group contents never travel over the own-device link: they come from the
//! hosts (U7), verified record by record (U10). Hosts' addresses for groups a device already has
//! are merged (newest signed address wins, active hosts only).

use std::sync::Arc;

use super::engine::Live;
use super::record::*;
use super::store::GroupMeta;
use crate::node::Inner;

/// At most this many groups per message.
pub const MAX_OWN_GROUPS: usize = 512;

impl Inner {
    /// Our groups for our other devices: those we are a current member of, with their hosts.
    pub(crate) fn group_own_tokens(self: &Arc<Self>) -> Vec<Vec<u8>> {
        let Ok(core) = self.group_core() else { return vec![] };
        let gids: Vec<(String, String, String)> = core
            .groups
            .lock()
            .iter()
            .filter(|(_, l)| l.state.owner.is_some() && l.state.is_member(&core.did) && !l.state.is_removed(&core.did) && l.meta.removal_proof.is_empty())
            .map(|(g, l)| (g.clone(), l.state.name.clone(), l.state.owner.clone().unwrap_or_default()))
            .collect();
        let mut out: Vec<Vec<u8>> = gids
            .into_iter()
            .map(|(gid, name, owner)| {
                // Chat hosts only (a call-only host refuses joiners); refresh our own address.
                let _ = self.group_my_addr(&core, &gid);
                let hosts = core.chat_host_addrs(&gid);
                bincode::serialize(&InviteToken { group: gid, name, owner, invite: None, hosts }).expect("token serialises")
            })
            .collect();
        out.sort();
        out.truncate(MAX_OWN_GROUPS);
        out
    }

    /// Groups one of our other devices is in: add the ones we lack, merge host addresses.
    pub(crate) fn group_own_apply(self: &Arc<Self>, tokens: &[Vec<u8>]) {
        let Ok(core) = self.group_core() else { return };
        let mut added = false;
        for b in tokens.iter().take(MAX_OWN_GROUPS) {
            let Ok(t) = bincode::deserialize::<InviteToken>(b) else { continue };
            if t.invite.is_some() || t.group.len() != 64 {
                continue;
            }
            let gid = t.group.clone();
            let have = core.groups.lock().contains_key(&gid);
            if have {
                if core.merge_hosts_pub(&gid, &t.hosts) {
                    added = true;
                }
                continue;
            }
            let hosts: Vec<&SignedHostAddr> = t.hosts.iter().filter(|h| h.open().is_some_and(|a| a.group == gid)).collect();
            if hosts.is_empty() {
                continue;
            }
            let meta = GroupMeta { from_own: true, ..Default::default() };
            let live = Live { doc: super::doc::GroupDoc::new(&gid, core.wid), entries: Vec::new(), state: Default::default(), meta: meta.clone() };
            core.groups.lock().insert(gid.clone(), live);
            if core.store.put_meta(&gid, &meta).is_err() {
                core.groups.lock().remove(&gid);
                continue;
            }
            for h in hosts {
                if let Some(a) = h.open() {
                    let _ = core.store.put_host(&gid, &a.did, &a.dev, h);
                }
            }
            self.log(format!("group {}: learned from an own device", &gid[..8]));
            added = true;
        }
        if added {
            let (this, core) = (self.clone(), core.clone());
            self.handle.spawn(async move {
                if let Err(e) = this.group_ensure(&core).await {
                    tracing::debug!("group endpoint: {e}");
                }
                core.kick_pub();
            });
        }
    }
}
