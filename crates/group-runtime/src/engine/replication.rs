//! Group frontiers, live fan-out, host addresses, and removal proofs.

use super::*;

impl GroupCore {
    /// The version vector of one of `gid`'s docs as we hold it (empty if we hold none of it).
    pub fn doc_vv(&self, gid: &str, doc: &DocId) -> VersionVector {
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else {
            return VersionVector::default();
        };
        self.doc_vv_of(live, gid, doc)
    }

    /// The recent days (see `HELLO_DAYS`) of `gid` we hold, with their version vectors.
    pub fn hello_days(&self, gid: &str) -> Vec<(String, Vec<u8>)> {
        recent_days(now_ms() as i64, HELLO_DAYS)
            .into_iter()
            .filter_map(|d| {
                let m = self.store.shard_meta(gid, &d).ok().flatten()?;
                Some((d, m.vv))
            })
            .collect()
    }

    /// Host addresses we pass on for `gid`: active chat and call hosts only (never a plain
    /// member's endpoint), ours included when we host either.
    pub(super) fn host_addrs(&self, gid: &str) -> Vec<SignedHostAddr> {
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else {
            return Vec::new();
        };
        let mut out: Vec<SignedHostAddr> = self
            .store
            .hosts(gid)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| {
                s.open().ok().is_some_and(|a| {
                    hosts_or_calls(&live.state, &a.did, &a.dev)
                        && !(a.did == self.did && a.dev == self.dev)
                })
            })
            .collect();
        if hosts_or_calls(&live.state, &self.did, &self.dev)
            && let Some(mine) = self.my_addr.lock().get(gid).cloned()
        {
            out.push(mine);
        }
        out
    }

    /// Sends every session of `gid` (except `from`) what it lacks of the meta doc and then of
    /// the day docs `days` (the meta doc always goes first, so a post never arrives before the
    /// records it names). Older days reach a peer through the inventory sync (`history.rs`), not through here.
    pub fn push(&self, gid: &str, from: Option<u64>, days: &[String]) {
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else {
            return;
        };
        let ours = live.doc.vv();
        // A session of a member removed just now still gets this last push (it carries the
        // flags that removed it); `group_enforce` closes it right after. It gets no posts.
        let sessions: Vec<Arc<Sess>> = self
            .sessions_of(gid)
            .into_iter()
            .filter(|s| Some(s.id) != from)
            .collect();
        for s in &sessions {
            let mut pv = s.peer_vv.lock();
            if let Some(update) = live.doc.export_since(&pv) {
                let _ = s.tx.send(Out::Msg(GroupMsg::Update {
                    doc: DocId::Meta,
                    update,
                }));
                pv.merge(&ours);
            }
        }
        let we_host = live.state.is_host(&self.did, &self.dev);
        for day in days {
            let Live {
                state,
                shards,
                meta,
                ..
            } = &mut *live;
            let Ok(shard) = shards.get(&self.store, gid, self.wid, day, state) else {
                continue;
            };
            let theirs = shard.doc.vv();
            for s in &sessions {
                if s.hosting && state.is_removed(&s.did) {
                    continue;
                }
                let mut pd = s.peer_days.lock();
                let pv = pd.entry(day.clone()).or_default();
                // To a host: never what it would refuse (admins-only, `admit::upload_cut`).
                let cut = if s.hosting || we_host {
                    Vec::new()
                } else {
                    admit::upload_cut(state, meta, &self.did, day, &shard.rows, pv)
                };
                if let Some(update) = shard.doc.export_since_before(pv, &cut) {
                    // With a cut the peer holds only what we sent, not the cut ops.
                    let sent = (!cut.is_empty())
                        .then(|| loro::LoroDoc::decode_import_blob_meta(&update, false).ok())
                        .flatten()
                        .map(|m| m.partial_end_vv);
                    let _ = s.tx.send(Out::Msg(GroupMsg::Update {
                        doc: DocId::Day(day.clone()),
                        update,
                    }));
                    match (cut.is_empty(), sent) {
                        (true, _) => pv.merge(&theirs),
                        (false, Some(v)) => pv.merge(&v),
                        (false, None) => {}
                    }
                }
            }
        }
    }

    /// The days of the last `HELLO_DAYS` we hold (what a new session catches the peer up on).
    pub fn held_recent_days(&self, gid: &str) -> Vec<String> {
        recent_days(now_ms() as i64, HELLO_DAYS)
            .into_iter()
            .filter(|d| self.store.shard_meta(gid, d).ok().flatten().is_some())
            .collect()
    }

    /// `merge_hosts`, and on a change pass the addresses on to every other session of `gid`
    /// (all but `from`) and wake the dialer. GF: every path that learns an address spreads it;
    /// before, only a `Hosts` frame did, so an address learned in a Hello/Welcome (e.g. a new
    /// host's, when its session to the first host was re-established right after it accepted)
    /// reached that host but never its members.
    pub(super) fn merge_hosts_from(
        &self,
        gid: &str,
        addrs: &[SignedHostAddr],
        from: Option<u64>,
    ) -> bool {
        if !self.merge_hosts(gid, addrs) {
            return false;
        }
        // The records that make these hosts valid go first, on each session's ordered stream.
        self.push(gid, None, &[]);
        let all = self.host_addrs(gid);
        for s in self.sessions_of(gid) {
            if Some(s.id) != from {
                let _ = s.tx.send(Out::Msg(GroupMsg::Hosts { hosts: all.clone() }));
            }
        }
        self.kick.notify_waiters();
        true
    }

    /// Stores host addresses that are newer and belong to active chat or call hosts (or, before
    /// we hold any state, i.e. while joining, any validly signed one for this group). True if any
    /// changed.
    pub(super) fn merge_hosts(&self, gid: &str, addrs: &[SignedHostAddr]) -> bool {
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else {
            return false;
        };
        let stored: HashMap<String, HostAddr> = self
            .store
            .hosts(gid)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| s.open().ok())
            .map(|a| (host_key(&a.did, &a.dev), a))
            .collect();
        let mut changed = false;
        for s in addrs.iter().take(64) {
            let Some(a) = s.open().ok() else { continue };
            if a.group != gid || (a.did == self.did && a.dev == self.dev) {
                continue;
            }
            if live.state.owner.is_some() && !hosts_or_calls(&live.state, &a.did, &a.dev) {
                continue;
            }
            if stored
                .get(&host_key(&a.did, &a.dev))
                .is_some_and(|old| old.at >= a.at)
            {
                continue;
            }
            if self.store.put_host(gid, &a.did, &a.dev, s).is_ok() {
                changed = true;
            }
        }
        changed
    }

    /// The signed records that prove `did` was removed: genesis, the owner's admin grants of the
    /// flaggers, and the counted flags.
    pub(super) fn removal_proof(&self, live: &Live, did: &str) -> Vec<Vec<u8>> {
        let Some(flags) = live.state.removed.get(did) else {
            return Vec::new();
        };
        let flag_ids: HashSet<&str> = flags.iter().map(|f| f.id.as_str()).collect();
        let flaggers: HashSet<&str> = flags.iter().map(|f| f.by.as_str()).collect();
        let owner = live.state.owner.clone().unwrap_or_default();
        let mut out = Vec::new();
        for e in &live.entries {
            let keep = match &e.rec.body {
                Body::Genesis { .. } => e.rec.author == owner,
                Body::Admin { member, .. } => {
                    e.rec.author == owner && flaggers.contains(member.as_str())
                }
                Body::Flag { .. } => flag_ids.contains(e.id.as_str()),
                _ => false,
            };
            if keep && let Some(b) = self.raw_record(live, &e.id) {
                out.push(b);
            }
        }
        out
    }

    /// What `did` (holding `theirs`) lacks of the causal past of the flags that removed it.
    pub(super) fn removal_upto(
        &self,
        live: &Live,
        did: &str,
        theirs: &VersionVector,
    ) -> Option<Vec<u8>> {
        let flags = live.state.removed.get(did)?;
        let ids: Vec<(u64, i32)> = flags
            .iter()
            .filter_map(|f| live.entries.iter().find(|e| e.id == f.id))
            .map(|e| (e.peer, e.counter))
            .collect();
        live.doc.export_until(&ids, theirs)
    }

    pub(super) fn raw_record(&self, live: &Live, id: &str) -> Option<Vec<u8>> {
        live.doc.raw_value(id)
    }

    /// Addresses for a token (invite link answer, direct invitation, own-device sync): active
    /// CHAT hosts only, ours included when we host the chat. A call-only host refuses joiners
    /// and fetches; the joiner learns call hosts from a chat host's Welcome.
    pub fn chat_host_addrs(&self, gid: &str) -> Vec<SignedHostAddr> {
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else {
            return Vec::new();
        };
        let mut out: Vec<SignedHostAddr> = self
            .store
            .hosts(gid)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| {
                s.open().ok().is_some_and(|a| {
                    live.state.is_host(&a.did, &a.dev) && !(a.did == self.did && a.dev == self.dev)
                })
            })
            .collect();
        if live.state.is_host(&self.did, &self.dev)
            && let Some(mine) = self.my_addr.lock().get(gid).cloned()
        {
            out.push(mine);
        }
        out
    }

    pub fn merge_hosts_pub(&self, gid: &str, addrs: &[SignedHostAddr]) -> bool {
        self.merge_hosts_from(gid, addrs, None)
    }

    pub fn kick_pub(&self) {
        self.kick.notify_waiters();
    }

    /// Sends `msg` on a session with one of `gid`'s active hosts (we are the member side);
    /// returns which host, or `None` if we have no such session.
    pub fn send_to_a_host(&self, gid: &str, msg: GroupMsg) -> Option<(String, [u8; 32])> {
        let hosts: Vec<(String, [u8; 32])> = self
            .groups
            .lock()
            .get(gid)
            .map(|l| {
                l.state
                    .hosts
                    .iter()
                    .map(|h| (h.did.clone(), h.dev))
                    .collect()
            })
            .unwrap_or_default();
        let s = self
            .sessions_of(gid)
            .into_iter()
            .find(|s| !s.hosting && hosts.contains(&(s.did.clone(), s.dev)))?;
        s.tx.send(Out::Msg(msg)).ok()?;
        Some((s.did.clone(), s.dev))
    }

    /// Sends `msg` on our session with host device `did`/`dev` of `gid`, if there is one.
    pub fn send_to_host(&self, gid: &str, did: &str, dev: &[u8; 32], msg: GroupMsg) -> bool {
        self.sessions_of(gid)
            .into_iter()
            .find(|s| !s.hosting && s.did == did && &s.dev == dev)
            .is_some_and(|s| s.tx.send(Out::Msg(msg)).is_ok())
    }
}
