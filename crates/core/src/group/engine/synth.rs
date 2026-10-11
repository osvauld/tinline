//! Test-only: builds big groups in-process for the year-scale measurements
//! (`docs/scale-year.md`). Members write signed records into their own docs; the batches are
//! merged in a hub doc and go through the real `GroupCore::apply` (validation, `rederive`,
//! `store.append`, post diff) and the search hook. Not used by the product.

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
    pub(crate) fn apply_for_test(&self, gid: &str, update: &[u8]) -> Result<Option<Changed>, Reject> {
        self.apply(gid, update, |_, _, _| Ok(()))
    }
}

impl Inner {
    /// A group owned by us (like `group_create`, without the endpoint), with its first records.
    pub(crate) fn synth_group_new(self: &Arc<Self>, name: &str) -> Result<SynthGroup, Error> {
        let core = self.group_core()?;
        let me = self.me()?;
        let nonce: [u8; 16] = rand::random();
        let gid = group_id(&core.did, &nonce);
        {
            let live = Live { doc: GroupDoc::new(&gid, core.wid), entries: Vec::new(), state: Default::default(), meta: GroupMeta::default() };
            core.groups.lock().insert(gid.clone(), live);
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
            self.search_group(&gid, &ch.posts);
        }
        let snap = core.groups.lock().get(&gid).ok_or(Error::NotFound)?.doc.snapshot();
        // Our own writing doc continues our counters from the doc as it is now.
        let owner_doc = GroupDoc::load(&gid, core.wid, Some(&snap), &[]).map_err(rej)?;
        let hub = GroupDoc::new(&gid, 0xFEED_0000_0000_0001);
        hub.import_for_test(&snap);
        let sent = hub.vv();
        let owner = SMember { did: core.did.clone(), acct: core.account.clone(), dev: core.dev, doc: owner_doc };
        Ok(SynthGroup { gid, hub, members: vec![owner], sent, owner_did: core.did.clone() })
    }

    /// The registered peer `did` joins (invited by us); returns its member index. Buffered until
    /// `synth_group_flush`.
    pub(crate) fn synth_group_add(self: &Arc<Self>, g: &mut SynthGroup, did: &str, name: &str, at: i64) -> Result<usize, Error> {
        let core = self.group_core()?;
        let (acct, dev) = crate::synth::peer(did).ok_or(Error::NotFound)?;
        let doc = GroupDoc::new(&g.gid, writer_id(did, &dev));
        let invite = Invite { group: g.gid.clone(), issuer: g.owner_did.clone(), expires: now() + 10 * 365 * 86_400, nonce: rand::random() }.sign(&core.account);
        g.members.push(SMember { did: did.to_string(), acct, dev, doc });
        let idx = g.members.len() - 1;
        self.synth_group_write(g, idx, at, Body::Join { invite, name: name.to_string() })?;
        Ok(idx)
    }

    pub(crate) fn synth_group_write(self: &Arc<Self>, g: &mut SynthGroup, idx: usize, at: i64, body: Body) -> Result<(), Error> {
        let m = &g.members[idx];
        let rec = Record { v: 1, group: g.gid.clone(), author: m.did.clone(), dev: m.dev, at, seen: g.hub.seen(), body };
        let (_, up) = m.doc.write(&m.acct, &rec).map_err(rej)?;
        g.hub.import_for_test(&up);
        Ok(())
    }

    /// Everything buffered goes through `apply` as one update, then the search hook.
    pub(crate) fn synth_group_flush(self: &Arc<Self>, g: &mut SynthGroup) -> Result<(), Error> {
        let core = self.group_core()?;
        let Some(update) = g.hub.export_since(&g.sent) else { return Ok(()) };
        g.sent = g.hub.vv();
        if let Some(changed) = core.apply_for_test(&g.gid, &update).map_err(rej)? {
            self.search_group(&g.gid, &changed.posts);
        }
        Ok(())
    }

    /// A member doc (peer `did`) loaded from the group as we hold it, to write remote records.
    pub(crate) fn synth_remote_open(self: &Arc<Self>, gid: &str, did: &str) -> Result<SynthRemote, Error> {
        let core = self.group_core()?;
        let (acct, dev) = crate::synth::peer(did).ok_or(Error::NotFound)?;
        let snap = core.groups.lock().get(gid).ok_or(Error::NotFound)?.doc.snapshot();
        let doc = GroupDoc::load(gid, writer_id(did, &dev), Some(&snap), &[]).map_err(rej)?;
        Ok(SynthRemote { gid: gid.to_string(), m: SMember { did: did.to_string(), acct, dev, doc } })
    }

    /// A post by a remote member arrives: `apply` (validation + rederive + store) and then
    /// `group_after` (search hook, relay, enforcement). Returns the time of each.
    pub(crate) fn synth_remote_post(self: &Arc<Self>, r: &mut SynthRemote, text: &str, at: i64) -> Result<(Duration, Duration), Error> {
        let core = self.group_core()?;
        let rec = Record { v: 1, group: r.gid.clone(), author: r.m.did.clone(), dev: r.m.dev, at, seen: r.m.doc.seen(), body: Body::Post { text: text.to_string(), reply_to: None } };
        let (_, up) = r.m.doc.write(&r.m.acct, &rec).map_err(rej)?;
        let t = Instant::now();
        let changed = core.apply_for_test(&r.gid, &up).map_err(rej)?.ok_or(Error::NotFound)?;
        let a = t.elapsed();
        let t = Instant::now();
        self.group_after(&core, changed, None);
        Ok((a, t.elapsed()))
    }
}
