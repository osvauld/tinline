//! Temporary home-screen banners expire without hiding form or setup errors.
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct NoticeTimer {
    shown: Option<(String, Instant)>,
}

impl NoticeTimer {
    pub fn update(&mut self, notice: &mut Option<String>, temporary: bool, now: Instant) {
        let Some(text) = notice.as_ref().filter(|_| temporary) else {
            self.shown = None;
            return;
        };
        if self.shown.as_ref().is_none_or(|(old, _)| old != text) {
            self.shown = Some((text.clone(), now));
        } else if self.shown.as_ref().is_some_and(|(_, since)| now.saturating_duration_since(*since) >= Duration::from_secs(8)) {
            *notice = None;
            self.shown = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_and_host_confirmations_expire_despite_background_updates() {
        for text in ["Device is busy", "This computer now hosts calls"] {
            let mut timer = NoticeTimer::default();
            let mut notice = Some(text.to_string());
            let now = Instant::now();
            for second in 0..8 {
                timer.update(&mut notice, true, now + Duration::from_secs(second));
                assert!(notice.is_some());
            }
            timer.update(&mut notice, true, now + Duration::from_secs(8));
            assert!(notice.is_none());
        }
    }

    #[test]
    fn new_message_gets_its_own_time_and_form_errors_do_not_expire() {
        let mut timer = NoticeTimer::default();
        let mut notice = Some("busy".into());
        let now = Instant::now();
        timer.update(&mut notice, true, now);
        notice = Some("hosting".into());
        timer.update(&mut notice, true, now + Duration::from_secs(7));
        timer.update(&mut notice, true, now + Duration::from_secs(8));
        assert_eq!(notice.as_deref(), Some("hosting"));
        timer.update(&mut notice, false, now + Duration::from_secs(60));
        assert_eq!(notice.as_deref(), Some("hosting"));
        notice = None;
        timer.update(&mut notice, true, now + Duration::from_secs(61));
        notice = Some("hosting".into());
        timer.update(&mut notice, true, now + Duration::from_secs(62));
        assert!(notice.is_some());
    }
}
