//! History arriving on a newly linked device updates the UI without replaying alerts.
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct MessageAlerts {
    since_ms: u64,
    last: Option<Instant>,
}

impl Default for MessageAlerts {
    fn default() -> Self {
        Self {
            since_ms: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            last: None,
        }
    }
}

impl MessageAlerts {
    /// Old history is silent; a burst of fresh messages plays at most one alert per second.
    /// Calls use their own notification path and are never throttled here.
    pub fn allow(&mut self, sent_ms: u64, now: Instant) -> bool {
        if sent_ms < self.since_ms || self.last.is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(1)) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linking_history_is_silent_without_consuming_the_next_live_alert() {
        let mut alerts = MessageAlerts { since_ms: 10_000, last: None };
        let now = Instant::now();
        for sent in 0..10_000 {
            assert!(!alerts.allow(sent, now));
        }
        assert!(alerts.allow(10_000, now));
    }

    #[test]
    fn live_burst_is_bounded_and_alerts_resume() {
        let mut alerts = MessageAlerts { since_ms: 10_000, last: None };
        let now = Instant::now();
        let count = (10_000..20_000).filter(|sent| alerts.allow(*sent, now)).count();
        assert_eq!(count, 1);
        assert!(alerts.allow(20_000, now + Duration::from_secs(1)));
    }
}
