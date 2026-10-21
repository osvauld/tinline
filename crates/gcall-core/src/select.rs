//! Speaker selection at the call host (§1.3, §6, G6-D2), over the RFC 6464 levels the senders
//! report. Pure; one [`Forwarder::frame`] per 20 ms, one [`Forwarder::offer`] per received
//! audio datagram.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// §1.3 speech threshold, -dBov (lower = louder). Same value as the sender's silence gate.
pub const SPEECH_DBOV: u8 = audio::SPEECH_LEVEL;
/// §1.3: 300 ms moving average.
pub const AVG_FRAMES: usize = 15;
/// §1.3 hysteresis: louder by this much, for `HYST_FRAMES` (200 ms).
pub const HYST_DB: u32 = 6;
pub const HYST_FRAMES: u64 = 10;
/// §6 dominance cap: more than `DOM_SHARE_PCT` of a rolling 60 s while contended demotes the
/// sender for 10 s.
pub const DOM_WINDOW: usize = 3_000;
pub const DOM_SHARE_PCT: usize = 90;
pub const DOM_DEMOTE_FRAMES: u64 = 500;

const SILENT: u32 = 127;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Provisional {
    slot: u8,
    since: u64,
    sum: u32,
    n: u32,
}

/// The active set S (at most K senders) plus at most one provisional slot (G6-D2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forwarder {
    k: usize,
    /// G6-D2 on/off. Off while a phone host is limited (K = 1).
    provisional_enabled: bool,
    frame: u64,
    hist: BTreeMap<u8, VecDeque<Option<u8>>>,
    set: BTreeSet<u8>,
    challenge: Option<(u8, u64)>,
    held: BTreeMap<u8, (VecDeque<bool>, usize)>,
    demoted_until: BTreeMap<u8, u64>,
    lowered: BTreeSet<u8>,
    prov: Option<Provisional>,
    /// Senders whose provisional slot was dropped; no new one until they go quiet.
    denied: BTreeSet<u8>,
}

impl Forwarder {
    pub fn new(k: usize) -> Self {
        Forwarder {
            k: k.max(1),
            provisional_enabled: true,
            frame: 0,
            hist: BTreeMap::new(),
            set: BTreeSet::new(),
            challenge: None,
            held: BTreeMap::new(),
            demoted_until: BTreeMap::new(),
            lowered: BTreeSet::new(),
            prov: None,
            denied: BTreeSet::new(),
        }
    }

    pub fn k(&self) -> usize {
        self.k
    }

    /// Change K (phone degrade). Shrinking drops the weakest members of S at once.
    pub fn set_k(&mut self, k: usize) {
        self.k = k.max(1);
        while self.set.len() > self.k {
            let avg = self.averages();
            let q = self.weakest(&avg);
            self.set.remove(&q);
        }
    }

    /// S, without the provisional slot.
    pub fn selected(&self) -> &BTreeSet<u8> {
        &self.set
    }

    pub fn provisional(&self) -> Option<u8> {
        self.prov.as_ref().map(|p| p.slot)
    }

    /// What the host forwards and announces in `Active`: S plus the provisional slot.
    pub fn forwarded(&self) -> BTreeSet<u8> {
        let mut f = self.set.clone();
        f.extend(self.provisional());
        f
    }

    pub fn is_forwarded(&self, slot: u8) -> bool {
        self.set.contains(&slot) || self.provisional() == Some(slot)
    }

    /// G6-D2 on or off; turning it off drops a provisional slot at once.
    pub fn set_provisional(&mut self, on: bool) {
        self.provisional_enabled = on;
        if !on {
            self.prov = None;
        }
    }

    /// The host user's "lower priority" for a participant (§6).
    pub fn set_lowered(&mut self, slot: u8, lowered: bool) {
        if lowered {
            self.lowered.insert(slot);
        } else {
            self.lowered.remove(&slot);
        }
    }

    /// The participant in `slot` left; forget everything about the slot.
    pub fn remove(&mut self, slot: u8) {
        self.hist.remove(&slot);
        self.set.remove(&slot);
        self.held.remove(&slot);
        self.demoted_until.remove(&slot);
        self.lowered.remove(&slot);
        self.denied.remove(&slot);
        if self.challenge.is_some_and(|(c, _)| c == slot) {
            self.challenge = None;
        }
        if self.provisional() == Some(slot) {
            self.prov = None;
        }
    }

    /// G6-D2: an audio datagram at `level` from `slot` arrived. A sender outside S that is at
    /// speech level gets the provisional slot at once, if it is free. Returns whether the
    /// forwarded set changed (the caller then sends `Active` before forwarding).
    pub fn offer(&mut self, slot: u8, level: u8) -> bool {
        if !self.provisional_enabled
            || level >= SPEECH_DBOV
            || self.set.contains(&slot)
            || self.prov.is_some()
            || self.low(slot)
            || self.denied.contains(&slot)
        {
            return false;
        }
        self.prov = Some(Provisional { slot, since: self.frame, sum: 0, n: 0 });
        true
    }

    fn low(&self, s: u8) -> bool {
        self.lowered.contains(&s) || self.demoted_until.get(&s).is_some_and(|&t| t > self.frame)
    }

    // G3: the mean of the levels that arrived inside the window; silent if none did.
    fn avg(&self, s: u8) -> u32 {
        let (sum, n) = self.hist[&s].iter().flatten().fold((0u32, 0u32), |(a, n), l| (a + *l as u32, n + 1));
        if n == 0 { SILENT } else { sum / n }
    }

    fn averages(&self) -> BTreeMap<u8, u32> {
        self.hist.keys().map(|s| (*s, self.avg(*s))).collect()
    }

    fn key(&self, avg: &BTreeMap<u8, u32>, s: u8) -> (bool, u32, u8) {
        (self.low(s), avg.get(&s).copied().unwrap_or(SILENT), s)
    }

    fn weakest(&self, avg: &BTreeMap<u8, u32>) -> u8 {
        *self.set.iter().max_by_key(|s| self.key(avg, **s)).expect("S is not empty")
    }

    /// One 20 ms frame. `levels` maps each slot that sent a datagram (audio or level-only)
    /// in this frame to its level.
    pub fn frame(&mut self, levels: &BTreeMap<u8, u8>) {
        self.frame += 1;
        let senders: BTreeSet<u8> = self.hist.keys().chain(levels.keys()).copied().collect();
        for s in &senders {
            let h = self.hist.entry(*s).or_default();
            h.push_back(levels.get(s).copied());
            while h.len() > AVG_FRAMES {
                h.pop_front();
            }
        }
        let avg = self.averages();
        let eligible: BTreeSet<u8> = senders.iter().copied().filter(|s| avg[s] < SPEECH_DBOV as u32).collect();
        self.set.retain(|s| eligible.contains(s));
        // Fill free slots in priority order; demoted or lowered senders come last.
        let mut waiting: Vec<u8> = eligible.difference(&self.set).copied().collect();
        waiting.sort_by_key(|s| self.key(&avg, *s));
        for s in waiting {
            if self.set.len() < self.k {
                self.set.insert(s);
            }
        }
        // Hysteresis: the best waiting sender replaces the weakest member of S once it has
        // been louder by HYST_DB for HYST_FRAMES. At most one swap per frame.
        let mut waiting: Vec<u8> = eligible.difference(&self.set).copied().collect();
        waiting.sort_by_key(|s| self.key(&avg, *s));
        match waiting.first() {
            None => self.challenge = None,
            Some(&c) => {
                let q = self.weakest(&avg);
                let swap = if self.low(c) {
                    false // a demoted or lowered sender never displaces anyone
                } else if self.low(q) {
                    true
                } else if avg[&c] + HYST_DB <= avg[&q] {
                    let run = match self.challenge {
                        Some((who, r)) if who == c => r + 1,
                        _ => 1,
                    };
                    self.challenge = Some((c, run));
                    run >= HYST_FRAMES
                } else {
                    self.challenge = None;
                    false
                };
                if swap {
                    self.set.remove(&q);
                    self.set.insert(c);
                    self.challenge = None;
                }
            }
        }
        // Dominance cap (§6). G4: contended = at least K other eligible senders.
        for s in &senders {
            let contended = eligible.iter().filter(|x| *x != s).count() >= self.k;
            let holds = self.set.contains(s);
            let (h, count) = self.held.entry(*s).or_default();
            let bit = holds && contended;
            h.push_back(bit);
            *count += usize::from(bit);
            while h.len() > DOM_WINDOW {
                *count -= usize::from(h.pop_front().unwrap_or(false));
            }
            if bit && *count * 100 > DOM_WINDOW * DOM_SHARE_PCT {
                h.clear();
                *count = 0;
                self.demoted_until.insert(*s, self.frame + DOM_DEMOTE_FRAMES);
            }
        }
        self.decide_provisional(levels, &avg);
        self.denied.retain(|s| avg.get(s).is_some_and(|a| *a < SPEECH_DBOV as u32));
    }

    // G6-D2: after HYST_FRAMES (200 ms) the provisional slot replaces the weakest member of S
    // if its spurt so far was louder by HYST_DB (or S has room and it was speech), or is
    // dropped. The normal rule may promote it earlier.
    fn decide_provisional(&mut self, levels: &BTreeMap<u8, u8>, avg: &BTreeMap<u8, u32>) {
        let Some(mut p) = self.prov.take() else { return };
        if self.set.contains(&p.slot) || !self.hist.contains_key(&p.slot) {
            return; // promoted by the normal rule, or gone
        }
        if let Some(l) = levels.get(&p.slot) {
            p.sum += *l as u32;
            p.n += 1;
        }
        if self.frame - p.since < HYST_FRAMES {
            self.prov = Some(p);
            return;
        }
        let spurt = if p.n == 0 { SILENT } else { p.sum / p.n };
        let promote = if self.low(p.slot) {
            None
        } else if self.set.len() < self.k {
            (spurt < SPEECH_DBOV as u32).then_some(None)
        } else {
            let q = self.weakest(avg);
            (self.low(q) || spurt + HYST_DB <= avg[&q]).then_some(Some(q))
        };
        match promote {
            Some(out) => {
                if let Some(q) = out {
                    self.set.remove(&q);
                }
                self.set.insert(p.slot);
                self.challenge = None;
            }
            None => {
                self.denied.insert(p.slot);
            }
        }
    }
}
