//! Presentation only. Call admission, encryption and media remain core-owned.
#[derive(Debug, Clone, Copy)]
pub enum Action { Minimize, Restore, About, CloseAbout }

#[derive(Default)]
pub struct State {
    id: Option<String>,
    pub minimized: bool,
    pub about: bool,
}
impl State {
    pub fn bind(&mut self, id: Option<&str>) {
        if self.id.as_deref() != id {
            self.id = id.map(str::to_owned);
            self.minimized = false;
            self.about = false;
        }
    }
    pub fn apply(&mut self, action: Action, can_minimize: bool) {
        if self.id.is_none() { return; }
        match action {
            Action::Minimize if can_minimize => { self.minimized = true; self.about = false; }
            Action::Minimize => {}
            Action::Restore => self.minimized = false,
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
mod tests {
    use super::*;
    #[test]
    fn presentation_resets_on_new_call_not_timer_updates() {
        let mut s = State::default(); s.bind(Some("a"));
        s.apply(Action::Minimize, true); s.bind(Some("a")); assert!(s.minimized);
        s.apply(Action::About, true); s.bind(Some("b")); assert!(!s.about && !s.minimized);
        s.bind(None); s.apply(Action::About, true); assert!(!s.about);
    }
    #[test]
    fn incoming_cannot_minimize_and_details_close_independently() {
        let mut s = State::default(); s.bind(Some("a"));
        s.apply(Action::Minimize, false); assert!(!s.minimized);
        s.apply(Action::About, false); s.apply(Action::CloseAbout, false); assert!(!s.about);
    }
    #[test]
    fn fixed_boxes_match_the_playground_flex_equations() {
        let g = Geometry::new(800.0);
        assert_eq!(g, Geometry { avatar_top: 209.0, controls_top: 674.0, text_top: 359.0 });
        assert_eq!(g.text_top - g.avatar_top, 150.0);
    }
}
