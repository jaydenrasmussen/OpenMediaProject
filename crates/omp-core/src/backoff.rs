//! Escalating back-off for providers (indexers, download clients, ...).
//! Ported from Radarr `ProviderStatusServiceBase` and `EscalationBackOff` (GPL-3.0).

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Seconds a provider stays disabled at each escalation level.
pub const PERIODS_SECONDS: [i64; 10] = [
    0,
    60,
    5 * 60,
    15 * 60,
    30 * 60,
    60 * 60,
    3 * 60 * 60,
    6 * 60 * 60,
    12 * 60 * 60,
    24 * 60 * 60,
];

const MAX_LEVEL: u32 = (PERIODS_SECONDS.len() - 1) as u32;

fn period(level: u32) -> Duration {
    Duration::seconds(PERIODS_SECONDS[level.min(MAX_LEVEL) as usize])
}

/// For this long after the process starts, failures do not escalate and the
/// disable window is capped (a network that is not up yet is not a bad indexer).
const STARTUP_GRACE: Duration = Duration::minutes(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub initial_failure: Option<DateTime<Utc>>,
    pub most_recent_failure: Option<DateTime<Utc>>,
    pub escalation_level: u32,
    pub disabled_till: Option<DateTime<Utc>>,
}

impl ProviderStatus {
    pub fn is_disabled(&self, now: DateTime<Utc>) -> bool {
        self.disabled_till.is_some_and(|t| t > now)
    }

    /// A success steps the level down by one (it does not reset it) and
    /// re-enables the provider. Returns false when nothing changed.
    pub fn record_success(&mut self) -> bool {
        if self.escalation_level == 0 {
            return false;
        }
        self.escalation_level -= 1;
        self.disabled_till = None;
        true
    }

    /// Records a failure.
    ///
    /// - `escalate`: false for plain connection failures, which hold the level.
    /// - `minimum_backoff`: e.g. a server's Retry-After; the level is raised
    ///   until the window is at least that long.
    /// - `process_start`: when this process started, for the startup grace.
    pub fn record_failure(
        &mut self,
        now: DateTime<Utc>,
        process_start: DateTime<Utc>,
        minimum_backoff: Option<Duration>,
        mut escalate: bool,
    ) {
        self.most_recent_failure = Some(now);
        if self.escalation_level == 0 {
            self.initial_failure = Some(now);
            self.escalation_level = 1;
            escalate = false;
        }

        let in_startup_grace = process_start + STARTUP_GRACE > now;
        // Radarr's minimum time since the initial failure is zero, so this is
        // never in a grace period of its own.
        let in_grace = self.initial_failure.is_some_and(|t| t > now);

        if escalate && !in_grace && !in_startup_grace {
            self.escalation_level = (self.escalation_level + 1).min(MAX_LEVEL);
        }

        let minimum = minimum_backoff.filter(|m| *m != Duration::zero());
        if let Some(minimum) = minimum {
            while self.escalation_level < MAX_LEVEL && period(self.escalation_level) < minimum {
                self.escalation_level += 1;
            }
        }

        if !in_grace || minimum.is_some() {
            self.disabled_till = Some(now + period(self.escalation_level));
        }

        if in_startup_grace
            && minimum.is_none()
            && let Some(till) = self.disabled_till
        {
            let cap = now + Duration::seconds(PERIODS_SECONDS[2]);
            if cap < till {
                self.disabled_till = Some(cap);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn t(minutes: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::minutes(minutes)
    }

    /// Process started long ago, so the startup grace is over.
    fn start() -> DateTime<Utc> {
        t(-1000)
    }

    #[test]
    fn first_failure_disables_for_a_minute_without_escalating() {
        let mut s = ProviderStatus::default();
        s.record_failure(t(0), start(), None, true);
        assert_eq!(s.escalation_level, 1);
        assert_eq!(s.initial_failure, Some(t(0)));
        assert_eq!(s.disabled_till, Some(t(1)));
        assert!(s.is_disabled(t(0)));
        assert!(!s.is_disabled(t(1)));
    }

    #[test]
    fn repeated_failures_climb_the_table() {
        let mut s = ProviderStatus::default();
        let expected_minutes = [1, 5, 15, 30, 60, 180, 360, 720, 1440, 1440];
        let mut now = 0;
        for (i, minutes) in expected_minutes.iter().enumerate() {
            s.record_failure(t(now), start(), None, true);
            assert_eq!(s.disabled_till, Some(t(now + minutes)), "failure {}", i + 1);
            now += 10_000;
        }
        assert_eq!(s.escalation_level, 9, "capped at the last level");
    }

    #[test]
    fn connection_failures_do_not_escalate() {
        let mut s = ProviderStatus::default();
        s.record_failure(t(0), start(), None, true);
        s.record_failure(t(10), start(), None, false);
        s.record_failure(t(20), start(), None, false);
        assert_eq!(s.escalation_level, 1);
    }

    #[test]
    fn success_steps_down_one_level_and_re_enables() {
        let mut s = ProviderStatus::default();
        assert!(!s.record_success(), "nothing to do at level 0");
        s.record_failure(t(0), start(), None, true);
        s.record_failure(t(2), start(), None, true);
        s.record_failure(t(10), start(), None, true);
        assert_eq!(s.escalation_level, 3);
        assert!(s.record_success());
        assert_eq!(s.escalation_level, 2, "stepped down, not reset");
        assert_eq!(s.disabled_till, None);
    }

    #[test]
    fn minimum_backoff_raises_the_level_until_long_enough() {
        let mut s = ProviderStatus::default();
        s.record_failure(t(0), start(), Some(Duration::minutes(20)), true);
        // Levels: 1 (1 min) < 20 min -> 2 (5) -> 3 (15) -> 4 (30 min).
        assert_eq!(s.escalation_level, 4);
        assert_eq!(s.disabled_till, Some(t(30)));
    }

    #[test]
    fn startup_grace_holds_the_level_and_caps_the_window() {
        let mut s = ProviderStatus::default();
        let started = t(-5); // 5 minutes ago: inside the 15-minute grace
        s.record_failure(t(0), started, None, true);
        s.record_failure(t(1), started, None, true);
        s.record_failure(t(2), started, None, true);
        assert_eq!(s.escalation_level, 1, "no escalation during startup");

        // A long Retry-After is honoured even then.
        let mut r = ProviderStatus::default();
        r.record_failure(t(0), started, Some(Duration::hours(2)), true);
        assert_eq!(
            r.disabled_till,
            Some(t(180)).filter(|_| r.escalation_level == 6)
        );
    }

    #[test]
    fn startup_cap_limits_a_previously_long_window() {
        let mut s = ProviderStatus {
            escalation_level: 8,
            initial_failure: Some(t(-1000)),
            ..Default::default()
        };
        s.record_failure(t(0), t(-5), None, false);
        assert_eq!(s.disabled_till, Some(t(5)), "capped to the 5-minute period");
    }
}
