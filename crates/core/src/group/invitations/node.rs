//! The `Node` API of direct group invitations, and its test hook.

use super::*;

#[uniffi::export]
impl Node {
    pub fn set_group_invitation_events(&self, events: Arc<dyn GroupInvitationEvents>) {
        *self.inner.group.inv_events.lock() = Some(events);
    }

    /// Admins: invites contacts `dids` to `group_id` over the private channel (one fresh signed
    /// 7-day invite each). Refused as a whole (`Rejected(reason)`) if any is not a contact, is
    /// blocked by us or blocks us, or is already a member. Asking again while an invitation to the
    /// same person is pending returns it (and retries its delivery).
    pub fn group_invite_contacts(&self, group_id: String, dids: Vec<String>) -> Result<Vec<GroupInvitation>, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            inner.group_admin_check(&core, &group_id)?;
            let mut dids = dids;
            dids.sort();
            dids.dedup();
            let mut names = Vec::new();
            {
                let s = inner.shared.lock();
                let groups = core.groups.lock();
                let l = groups.get(&group_id).ok_or(Error::NotFound)?;
                for did in &dids {
                    let c = s.state.contacts.iter().find(|c| &c.did == did);
                    let Some(c) = c else { return Err(Error::Rejected(format!("not a contact: {did}"))) };
                    let name = crate::node::display_name(c);
                    if s.state.blocked.contains(did) {
                        return Err(Error::Rejected(format!("you block {name}")));
                    }
                    if crate::block::blocked_me(&s.state, did) {
                        return Err(Error::Rejected(format!("{name} blocked you")));
                    }
                    if l.state.is_member(did) {
                        return Err(Error::Rejected(format!("{name} is already a member")));
                    }
                    names.push(name);
                }
            }
            inner.group_ensure(&core).await?;
            inner.group_relay_ready().await;
            let my_name = inner.me()?.profile.name.clone();
            let existing = core.store.invs()?;
            let mut out = Vec::new();
            for (did, name) in dids.iter().zip(names) {
                let pending = existing.iter().find(|r| r.outgoing && &r.invitee == did && r.group == group_id && r.state == GroupInvitationState::Pending && r.expires >= now());
                let row = match pending {
                    Some(r) => r.clone(),
                    None => {
                        let invite = issue_invite(&core.account, &group_id, now() + INVITE_TTL);
                        let t = inner.group_token(&core, &group_id, Some(invite))?;
                        let row = InvRow {
                            id: hex::encode(cryptography::rng::bytes::<16>()),
                            group: group_id.clone(),
                            group_name: t.name.clone(),
                            inviter: core.did.clone(),
                            inviter_name: my_name.clone(),
                            invitee: did.clone(),
                            invitee_name: name,
                            outgoing: true,
                            state: GroupInvitationState::Pending,
                            at: now_ms(),
                            expires: now() + INVITE_TTL,
                            token: cryptography::codec::to_bytes(&t).expect("token serialises"),
                            delivered: false,
                            owe: true,
                        };
                        inner.inv_save(&core, &row);
                        row
                    }
                };
                out.push(inner.inv_view(&row));
                inner.invites_kick(did.clone());
            }
            Ok(out)
        })?
    }

    /// Invitations to groups we got (from DIDs we do not block), newest first.
    pub fn group_invitations_incoming(&self) -> Result<Vec<GroupInvitation>, Error> {
        let blocked = self.inner.shared.lock().state.blocked.clone();
        self.inner.inv_list(move |r| !r.outgoing && !blocked.contains(&r.inviter))
    }

    /// Invitations we sent for `group_id`, newest first.
    pub fn group_invitations_outgoing(&self, group_id: String) -> Result<Vec<GroupInvitation>, Error> {
        self.inner.inv_list(move |r| r.outgoing && r.group == group_id)
    }

    /// Joins the group of an incoming invitation (blocks until joined, like `group_join`; the same
    /// checks). `Rejected` if it was answered already or ran out.
    pub fn group_invitation_accept(&self, id: String) -> Result<GroupInvitation, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            let r = core.store.inv(&id)?.ok_or(Error::NotFound)?;
            let mut r = inner.inv_current(&core, r);
            if r.outgoing {
                return Err(Error::Rejected("that is our own invitation".into()));
            }
            match r.state {
                GroupInvitationState::Pending => {}
                GroupInvitationState::Joined => return Ok(inner.inv_view(&r)),
                GroupInvitationState::Expired => return Err(Error::Rejected("this invitation has expired".into())),
                _ => return Err(Error::Rejected("already answered".into())),
            }
            let t: InviteToken = cryptography::codec::from_bytes(&r.token, crate::group::record::MAX_TOKEN).map_err(|_| Error::Protocol("bad invitation".into()))?;
            inner.group_join_token(t).await?;
            r.state = GroupInvitationState::Joined;
            r.owe = false;
            inner.inv_save(&core, &r);
            Ok(inner.inv_view(&r))
        })?
    }

    /// Declines an incoming invitation; the inviter sees Declined (delivered when it is next
    /// reachable).
    pub fn group_invitation_decline(&self, id: String) -> Result<GroupInvitation, Error> {
        let core = self.inner.group_core()?;
        let r = core.store.inv(&id)?.ok_or(Error::NotFound)?;
        let mut r = self.inner.inv_current(&core, r);
        if r.outgoing {
            return Err(Error::Rejected("that is our own invitation".into()));
        }
        if r.state != GroupInvitationState::Pending {
            return Err(Error::Rejected("already answered".into()));
        }
        r.state = GroupInvitationState::Declined;
        r.owe = true;
        self.inner.inv_save(&core, &r);
        self.inner.invites_kick(r.inviter.clone());
        Ok(self.inner.inv_view(&r))
    }
}

/// Test hooks (not for apps).
#[uniffi::export]
impl Node {
    /// The group endpoint ids of the hosts the token of invitation `id` names.
    pub fn group_invitation_hosts_for_test(&self, id: String) -> Vec<String> {
        let Ok(core) = self.inner.group_core() else {
            return vec![];
        };
        let row = core
            .store
            .invs()
            .unwrap_or_default()
            .into_iter()
            .find(|r| r.id == id);
        let Some(t) = row.and_then(|r| {
            cryptography::codec::from_bytes::<InviteToken>(
                &r.token,
                crate::group::record::MAX_TOKEN,
            )
            .ok()
        }) else {
            return vec![];
        };
        t.hosts
            .iter()
            .filter_map(|h| h.open().ok())
            .filter_map(|a| iroh::PublicKey::from_bytes(&a.endpoint).ok())
            .map(|k| k.to_string())
            .collect()
    }
}
