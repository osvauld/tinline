//! Find in chat (`docs/search.md` "Find in chat"): the filter a [`Scan`](super::index::Scan)
//! applies to walk one conversation only. Segments whose manifest days do not include the
//! conversation are never opened; inside a segment the sealed doc table `z/` (one u16 per doc:
//! the index into the segment's `days`) drops other conversations' postings without reading
//! their rows.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::seg::{Seg, View, Z_NONE, decode_z};

/// Postings of other conversations dropped per query at most (they are cheap: no row is read).
pub const DROP_BUDGET: usize = 200_000;

/// What the filter says of one posting.
pub enum Verdict {
    /// Read the row.
    Keep,
    /// Another conversation's posting.
    Drop,
    /// The drop budget ran out: end the page here.
    Stop,
}

pub struct Filter {
    /// Keyed hash of the conversation's src.
    pub src: u64,
    budget: usize,
    /// Postings dropped so far.
    pub dropped: usize,
    /// Segments opened so far.
    pub opened: usize,
    /// Doc table parts read so far.
    pub z_reads: usize,
    /// Per open segment: for each of its days, whether it is this conversation's.
    mine: HashMap<u32, Vec<bool>>,
    z: HashMap<(u32, usize), Option<Arc<Vec<u16>>>>,
    /// Pseudo-postings that stand for "the budget ran out here".
    pub stops: HashSet<(u64, u64)>,
    /// The cursor to continue from, once a stop was reached.
    pub stopped: Option<String>,
}

impl Filter {
    pub fn new(src: u64, drop_budget: usize) -> Self {
        Self { src, budget: drop_budget, dropped: 0, opened: 0, z_reads: 0, mine: HashMap::new(), z: HashMap::new(), stops: HashSet::new(), stopped: None }
    }

    /// Whether the segment can hold the conversation's docs: it owns one of its days.
    pub fn wants(&self, view: &View, s: &Seg) -> bool {
        s.days.iter().any(|(a, d, _)| *a == self.src && view.owner.get(&(*a, *d)).map(|o| o.0) == Some(s.id))
    }

    pub fn open(&mut self, s: &Seg) {
        self.opened += 1;
        self.mine.insert(s.id, s.days.iter().map(|(a, _, _)| *a == self.src).collect());
    }

    /// The verdict on doc `doc` of segment `seg`; `read` fetches the sealed record of a table
    /// part (`None`: absent, then every posting is kept and the row decides).
    pub fn check(&mut self, seg: u32, doc: u32, read: &mut dyn FnMut(usize) -> Option<Vec<u8>>) -> Verdict {
        let part = doc as usize / super::seg::Z_PART;
        let table = match self.z.get(&(seg, part)) {
            Some(t) => t.clone(),
            None => {
                self.z_reads += 1;
                let t = read(part).and_then(|b| decode_z(&b)).map(Arc::new);
                self.z.insert((seg, part), t.clone());
                t
            }
        };
        let Some(day) = table.and_then(|t| t.get(doc as usize % super::seg::Z_PART).copied()) else { return Verdict::Keep };
        if day == Z_NONE {
            return Verdict::Keep;
        }
        if self.mine.get(&seg).and_then(|m| m.get(day as usize)).copied().unwrap_or(true) {
            return Verdict::Keep;
        }
        self.dropped += 1;
        if self.dropped > self.budget { Verdict::Stop } else { Verdict::Drop }
    }
}

#[cfg(test)]
mod tests;
