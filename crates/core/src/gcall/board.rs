//! A member's view of `CallStart` announcements (§3.1, §3.2, §5): generations, glare, split
//! after redial, which call is shown, and whether this device may take over as a backup.

use std::collections::BTreeMap;

use proto::SignedBlob;
use proto::gcall::{self as w, CallStart, EndReason, HostRef};

use super::*;

/// §3.1 glare: the lower `BLAKE3(call_id)` wins.
pub fn call_hash(call_id: &str) -> [u8; 32] {
    *blake3::hash(call_id.as_bytes()).as_bytes()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallBoard {
    gid: String,
    /// This device's call-host ref, if it holds a `host_call` permit.
    me: Option<HostRef>,
    hgen: BTreeMap<String, u32>,
    starts: BTreeMap<String, CallStart>,
    prev: BTreeMap<String, CallStart>,
    shown: Option<String>,
}

impl CallBoard {
    pub fn new(gid: &str, me: Option<HostRef>) -> Self {
        CallBoard { gid: gid.into(), me, hgen: BTreeMap::new(), starts: BTreeMap::new(), prev: BTreeMap::new(), shown: None }
    }

    /// A relayed (or directly received) CallStart: §3.2 items 1–3, generation, split and
    /// glare. A refusal leaves the board unchanged.
    pub fn accept_start(&mut self, blob: &SignedBlob, auth: &dyn CallAuthority) -> Result<(), Refusal> {
        let s = w::open_start(blob)?;
        if s.gid != self.gid {
            return Err(Refusal::WrongGroup);
        }
        if !auth.host_permit_ok(&s.gid, &s.host_ref) {
            return Err(Refusal::PermitInvalid);
        }
        if !auth.is_member(&s.gid, &s.starter_did) {
            return Err(Refusal::NotMember);
        }
        let cur = self.hgen.get(&s.call_id).copied().unwrap_or(0);
        if s.host_gen < cur {
            return Err(Refusal::StaleGen);
        }
        if s.host_gen == cur
            && let Some(have) = self.starts.get(&s.call_id)
        {
            if have.host_ref == s.host_ref {
                return Err(Refusal::Duplicate);
            }
            // Split after redial (§5): the lower index in the previous backups wins; hosts
            // outside that list fall back to the §2.3 tie-hash (`rank::tie_hash`).
            let order = self.prev.get(&s.call_id).map(|p| p.backups.clone()).unwrap_or_default();
            let idx = |r: &HostRef| order.iter().position(|b| b == r).unwrap_or(usize::MAX);
            if (idx(&s.host_ref), rank::tie_hash(&s.call_id, &s.host_ref)) >= (idx(&have.host_ref), rank::tie_hash(&s.call_id, &have.host_ref)) {
                return Err(Refusal::Superseded);
            }
        }
        let call_id = s.call_id.clone();
        if s.host_gen > cur
            && let Some(old) = self.starts.remove(&call_id)
        {
            self.prev.insert(call_id.clone(), old);
        }
        self.hgen.insert(call_id.clone(), s.host_gen);
        self.starts.insert(call_id, s);
        self.reshow();
        Ok(())
    }

    /// A relayed `End{ended}` removes the call from the board. Other reasons only concern
    /// participants and are ignored here.
    pub fn accept_end(&mut self, blob: &SignedBlob, auth: &dyn CallAuthority) -> Result<(), Refusal> {
        let e = w::open_end(blob)?;
        if e.gid != self.gid {
            return Err(Refusal::WrongGroup);
        }
        let cur = self.hgen.get(&e.call_id).copied().unwrap_or(0);
        if e.host_gen < cur {
            return Err(Refusal::StaleGen);
        }
        let s = self.starts.get(&e.call_id).filter(|s| s.host_gen == e.host_gen).ok_or(Refusal::Unknown)?;
        if s.host_ref.holder_did != e.host_did {
            return Err(Refusal::NotHost);
        }
        if !auth.host_permit_ok(&e.gid, &s.host_ref) {
            return Err(Refusal::PermitInvalid);
        }
        if e.reason == EndReason::Ended {
            self.starts.remove(&e.call_id);
            self.reshow();
        }
        Ok(())
    }

    fn reshow(&mut self) {
        self.shown = self.starts.keys().min_by_key(|c| call_hash(c)).cloned();
    }

    /// "Call in progress": the glare winner among live calls of the group.
    pub fn shown(&self) -> Option<&CallStart> {
        self.shown.as_ref().and_then(|c| self.starts.get(c))
    }

    pub fn start(&self, call_id: &str) -> Option<&CallStart> {
        self.starts.get(call_id)
    }

    /// The generation this device may host on a first valid Join: it is a backup of the
    /// current CallStart.
    pub fn may_take(&self, call_id: &str) -> Option<u32> {
        let me = self.me.as_ref()?;
        let s = self.starts.get(call_id)?;
        s.backups.contains(me).then_some(s.host_gen + 1)
    }

    /// The live call hosted by `host` (to follow an `End{superseded, next_host}`).
    #[cfg(test)]
    pub fn hosted_by(&self, host: &HostRef) -> Option<&CallStart> {
        self.starts.values().find(|s| s.host_ref == *host)
    }

    /// If this device hosts `hosting` and that call lost a glare or split, the winner's host
    /// (send `End{superseded, next_host: winner}`).
    pub fn superseded(&self, hosting: &CallStart) -> Option<HostRef> {
        let w = self.shown()?;
        (w.host_ref != hosting.host_ref && (w.call_id != hosting.call_id || w.host_gen == hosting.host_gen)).then(|| w.host_ref.clone())
    }
}
