//! Test-only: builds big groups in-process for the year-scale measurements
//! (`docs/scale-year.md`). Members write signed records into their own docs (posts into the
//! day docs of their time, the rest into meta); the batches are merged in hub docs and go through
//! the real `GroupCore::apply` (validation, `rederive`, `store.append`, post diff) and the search
//! hook. Not used by the product.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::synth::{SMember, SynthGroup, SynthRemote};

fn rej(r: Reject) -> Error {
    Error::Protocol(r.to_string())
}

impl GroupCore {
    /// `apply` with a policy that admits everything (what a member's session does, minus the
    /// host-side joiner check).
    pub(crate) fn apply_for_test(&self, gid: &str, doc: &DocId, update: &[u8]) -> Result<Option<Changed>, Reject> {
        self.apply(gid, doc, update, |_, _, _| Ok(()))
    }
}

impl Inner {
    /// A group owned by us (like `group_create`, without the endpoint), with its first records.
    pub(crate) fn synth_group_new(self: &Arc<Self>, name: &str) -> Result<SynthGroup, Error> {
        let core = self.group_core()?;
        let me = self.me()?;
        let nonce: [u8; 16] = cryptography::rng::bytes();
        let gid = group_id(&core.did, &nonce);
        {
            core.groups.lock().insert(gid.clone(), Live::empty(&gid, core.wid, GroupMeta::default()));
            core.store.put_meta(&gid, &GroupMeta::default())?;
        }
        let owner_name = me.profile.name.clone();
        let key = core.account.encryption_public_key();
        for body in [
            Body::Genesis { name: name.to_string(), nonce, owner_name },
            Body::HostPermit { host: core.did.clone(), host_dev: core.dev, active: true },
            Body::HostAccept { active: true },
            Body::EncKey { key },
        ] {
            let (_, ch) = core.write(&gid, body)?;
            self.search.group(&gid, &ch.posts);
        }
        let snap = core.groups.lock().get(&gid).ok_or(Error::NotFound)?.doc.snapshot();
        // Our own writing doc continues our counters from the doc as it is now.
        let owner_doc = GroupDoc::load(&gid, core.wid, Some(&snap), &[]).map_err(rej)?;
        let hub = GroupDoc::new(&gid, 0xFEED_0000_0000_0001);
        hub.import_for_test(&snap);
        let sent = hub.vv();
        let owner = SMember {
            did: core.did.clone(),
            acct: core.account.clone(),
            dev: core.dev,
            doc: owner_doc,
            days: Default::default(),
        };
        Ok(SynthGroup {
            gid,
            hub,
            day_hubs: Default::default(),
            members: vec![owner],
            sent,
            day_sent: Default::default(),
        })
    }

    /// The registered peer `did` joins (invited by us); returns its member index. Buffered until
    /// `synth_group_flush`.
    pub(crate) fn synth_group_add(self: &Arc<Self>, g: &mut SynthGroup, did: &str, name: &str, at: i64) -> Result<usize, Error> {
        let core = self.group_core()?;
        let (acct, dev) = crate::synth::peer(did).ok_or(Error::NotFound)?;
        let doc = GroupDoc::new(&g.gid, writer_id(did, &dev));
        let invite = issue_invite(&core.account, &g.gid, crate::node::now_ms(), super::INVITE_TTL);
        g.members.push(SMember {
            did: did.to_string(),
            acct,
            dev,
            doc,
            days: Default::default(),
        });
        let idx = g.members.len() - 1;
        self.synth_group_write(g, idx, at, Body::Join { invite, name: name.to_string() })?;
        Ok(idx)
    }

    pub(crate) fn synth_group_write(self: &Arc<Self>, g: &mut SynthGroup, idx: usize, at: i64, body: Body) -> Result<(), Error> {
        let gid = g.gid.clone();
        let m = &mut g.members[idx];
        let rec = Record {
            group: gid.clone(),
            author: m.did.clone(),
            dev: m.dev,
            at,
            seen: g.hub.seen(),
            body,
        };
        if matches!(rec.body, Body::Post { .. }) {
            let day = day_of(at);
            let wid = writer_id(&m.did, &m.dev);
            let doc = m.days.entry(day.clone()).or_insert_with(|| GroupDoc::new_in(&gid, DocId::Day(day.clone()), wid));
            let (_, up) = doc.write(&m.acct, &rec).map_err(rej)?;
            g.day_hubs.entry(day.clone()).or_insert_with(|| GroupDoc::new_in(&gid, DocId::Day(day), 0xFEED_0000_0000_0001)).import_for_test(&up);
        } else {
            let (_, up) = m.doc.write(&m.acct, &rec).map_err(rej)?;
            g.hub.import_for_test(&up);
        }
        Ok(())
    }

    /// Everything buffered goes through `apply`, the meta doc first and then each day doc, as
    /// one update each, then the search hook.
    pub(crate) fn synth_group_flush(self: &Arc<Self>, g: &mut SynthGroup) -> Result<(), Error> {
        self.synth_group_flush_part(g, true, true)
    }

    /// `synth_group_flush`, only the meta doc and/or only the day docs (a post that arrives
    /// before the records it names).
    pub(crate) fn synth_group_flush_part(self: &Arc<Self>, g: &mut SynthGroup, meta: bool, days: bool) -> Result<(), Error> {
        let core = self.group_core()?;
        if !meta {
            return self.synth_flush_days(&core, g, days);
        }
        if let Some(update) = g.hub.export_since(&g.sent) {
            g.sent = g.hub.vv();
            if let Some(changed) = core.apply_for_test(&g.gid, &DocId::Meta, &update).map_err(rej)? {
                self.search.group(&g.gid, &changed.posts);
            }
        }
        self.synth_flush_days(&core, g, days)
    }

    fn synth_flush_days(self: &Arc<Self>, core: &Arc<GroupCore>, g: &mut SynthGroup, days: bool) -> Result<(), Error> {
        if !days {
            return Ok(());
        }
        let mut names: Vec<&String> = g.day_hubs.keys().collect();
        names.sort();
        for day in names {
            let hub = &g.day_hubs[day];
            let sent = g.day_sent.entry(day.clone()).or_default();
            let Some(update) = hub.export_since(sent) else { continue };
            *sent = hub.vv();
            if let Some(changed) = core.apply_for_test(&g.gid, &DocId::Day(day.clone()), &update).map_err(rej)? {
                self.search.group(&g.gid, &changed.posts);
            }
        }
        Ok(())
    }

    /// A member doc (peer `did`) loaded from the group as we hold it, to write remote records.
    pub(crate) fn synth_remote_open(self: &Arc<Self>, gid: &str, did: &str) -> Result<SynthRemote, Error> {
        let core = self.group_core()?;
        let (acct, dev) = crate::synth::peer(did).ok_or(Error::NotFound)?;
        let snap = core.groups.lock().get(gid).ok_or(Error::NotFound)?.doc.snapshot();
        let doc = GroupDoc::load(gid, writer_id(did, &dev), Some(&snap), &[]).map_err(rej)?;
        Ok(SynthRemote { gid: gid.to_string(), m: SMember { did: did.to_string(), acct, dev, doc, days: Default::default() } })
    }

    /// A post by a remote member arrives: `apply` (validation + rederive + store) and then
    /// `group_after` (search hook, relay, enforcement). Returns the time of each.
    pub(crate) fn synth_remote_post(self: &Arc<Self>, r: &mut SynthRemote, text: &str, at: i64) -> Result<(Duration, Duration), Error> {
        let core = self.group_core()?;
        let day = day_of(at);
        if !r.m.days.contains_key(&day) {
            // The day as we hold it, so the member's counters continue.
            let snap = {
                let mut groups = core.groups.lock();
                let Live { state, shards, .. } = groups.get_mut(&r.gid).ok_or(Error::NotFound)?;
                shards.get(&core.store, &r.gid, core.wid, &day, state)?.doc.snapshot()
            };
            let doc = GroupDoc::load_in(&r.gid, DocId::Day(day.clone()), writer_id(&r.m.did, &r.m.dev), Some(&snap), &[]).map_err(rej)?;
            r.m.days.insert(day.clone(), doc);
        }
        let rec = Record {
            group: r.gid.clone(),
            author: r.m.did.clone(),
            dev: r.m.dev,
            at,
            seen: r.m.doc.seen(),
            body: Body::Post {
                text: text.to_string(),
                reply_to: None,
                image: None,
            },
        };
        let (_, up) = r.m.days[&day].write(&r.m.acct, &rec).map_err(rej)?;
        let t = Instant::now();
        let changed = core.apply_for_test(&r.gid, &DocId::Day(day), &up).map_err(rej)?.ok_or(Error::NotFound)?;
        let a = t.elapsed();
        let t = Instant::now();
        self.group_after(&core, changed, None);
        Ok((a, t.elapsed()))
    }
}
