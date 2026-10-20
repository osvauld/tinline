//! Presentation only. Call admission, encryption and media remain core-owned.
#[derive(Debug, Clone, Copy)]
pub enum Action { Minimize, Restore, About, CloseAbout }

#[derive(Default)]
pub struct State {
    id: Option<String>,
    pub minimized: bool,
    pub about: bool,
    /// IncomingAlert: the incoming call was opened to the full screen (until then it is the card, top right).
    pub expanded: bool,
    /// Recent loss for the signal (link.svelte.ts): resets with each call.
    pub loss: Loss,
}

/// Signal bars 1-4 (thresholds and words: `present::call::call_quality`).
/// 4 Strong rtt < 100 and loss < 1 % · 3 Good < 200 / < 3 % · 2 Fair < 350 / < 8 % · 1 Weak.
pub fn strength(rtt_ms: u32, loss_pct: f64) -> u8 {
    present::call::call_quality(rtt_ms, loss_pct).bars
}

/// A loss percentage the way the card prints it: "0", "0.2", "14".
pub fn pct(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 || r >= 10.0 { format!("{}", r.round()) } else { format!("{r:.1}") }
}

/// The word for a strength (1-4).
pub fn signal_word(level: u8) -> String {
    present::call::signal_word(level)
}

/// Loss over the last few seconds, not the whole call, or one bad minute would colour the rest of it.
/// Fed `CallStats.received/lost` (running totals) about once a second.
#[derive(Default)]
pub struct Loss { seen: std::collections::VecDeque<(u64, u64)> }
impl Loss {
    const WINDOW: usize = 5;
    pub fn push(&mut self, received: u64, lost: u64) {
        if self.seen.back().is_some_and(|&(r, l)| r > received || l > lost) { self.seen.clear(); }
        self.seen.push_back((received, lost));
        while self.seen.len() > Self::WINDOW + 1 { self.seen.pop_front(); }
    }
    pub fn pct(&self) -> f64 {
        let (Some(&(r0, l0)), Some(&(r1, l1))) = (self.seen.front(), self.seen.back()) else { return 0.0 };
        let (r, l) = (r1 - r0, l1 - l0);
        if r + l == 0 { 0.0 } else { l as f64 * 100.0 / (r + l) as f64 }
    }
}
impl State {
    pub fn bind(&mut self, id: Option<&str>) {
        if self.id.as_deref() != id {
            self.id = id.map(str::to_owned);
            self.minimized = false;
            self.about = false;
            self.expanded = false;
            self.loss = Loss::default();
        }
    }
    pub fn apply(&mut self, action: Action, can_minimize: bool) {
        if self.id.is_none() { return; }
        match action {
            Action::Minimize if can_minimize => { self.minimized = true; self.about = false; }
            Action::Minimize => {}
            Action::Restore => { self.minimized = false; self.expanded = true; }
            Action::About => self.about = true,
            Action::CloseAbout => self.about = false,
        }
    }
}

/// CallScreen.svelte's flex layout, independent of every changing string.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry { pub avatar_top: f32, pub controls_top: f32, pub text_top: f32 }
impl Geometry {
    pub fn new(height: f32) -> Self {
        // top: 8 + 48; bottom: 36 + (64 + 8 + 18); who padding-bottom: 8cqb.
        let controls_top = height - 126.0;
        let avatar_top = (56.0 + (height - 182.0 - height * 0.08 - 248.0) / 2.0).max(56.0);
        Self { avatar_top, controls_top, text_top: avatar_top + 150.0 }
    }
}

#[cfg(test)]
#[path = "call_ui_tests.rs"]
mod tests;
