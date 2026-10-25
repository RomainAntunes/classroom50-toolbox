//! Short cooldowns for the read-only checks behind Settings' and the list's buttons, so a
//! burst of clicks reuses the last answer instead of running gh or asking GitHub each time.
//! Writes (accept, submit, pull, push, clone) never go through here.

use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};

/// A result and when it was produced.
#[derive(Debug, Clone)]
pub struct Recent<T> {
    at: Instant,
    value: T,
}

impl<T> Recent<T> {
    pub fn new(at: Instant, value: T) -> Self {
        Self { at, value }
    }

    /// The value while it's younger than `window` at `now`.
    pub fn fresh(&self, now: Instant, window: Duration) -> Option<&T> {
        (now.saturating_duration_since(self.at) < window).then_some(&self.value)
    }
}

/// Whether `at` (wall clock) is less than `window` before `now`. A time ahead of `now` (the
/// clock went back) doesn't count.
pub fn within(at: Option<DateTime<Utc>>, now: DateTime<Utc>, window: TimeDelta) -> bool {
    at.is_some_and(|at| at <= now && now - at < window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_is_fresh_inside_its_window_only() {
        let start = Instant::now();
        let recent = Recent::new(start, 7);
        let window = Duration::from_secs(3);
        assert_eq!(recent.fresh(start, window), Some(&7));
        assert_eq!(
            recent.fresh(start + Duration::from_millis(2999), window),
            Some(&7)
        );
        assert_eq!(recent.fresh(start + window, window), None);
        assert_eq!(recent.fresh(start + Duration::from_secs(60), window), None);
    }

    #[test]
    fn within_counts_only_the_past_window() {
        let now = DateTime::parse_from_rfc3339("2026-10-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let window = TimeDelta::seconds(5);
        assert!(!within(None, now, window));
        assert!(within(Some(now), now, window));
        assert!(within(Some(now - TimeDelta::seconds(4)), now, window));
        assert!(!within(Some(now - TimeDelta::seconds(5)), now, window));
        assert!(!within(Some(now + TimeDelta::seconds(1)), now, window));
    }
}
