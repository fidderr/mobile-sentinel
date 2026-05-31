use serde::{Deserialize, Serialize};

/// Configurable snooze behavior for a Recipe instance.
///
/// `max_count == 0` disables snooze entirely. Subsequent snoozes within
/// the same Firing session use [`Self::interval_for`] which optionally
/// applies an escalation strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnoozePolicy {
    /// Maximum snoozes allowed in one Firing session.
    pub max_count: u32,
    /// Base interval in minutes between fire and next fire.
    pub interval_minutes: u32,
    /// Optional escalation strategy applied to subsequent snoozes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation: Option<EscalationPolicy>,
}

/// Strategy for growing the snooze interval across successive snoozes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EscalationPolicy {
    /// `interval + index * step_minutes`.
    Linear { step_minutes: u32 },
    /// `interval * factor.powi(index)` rounded down to integer minutes.
    Exponential { factor: f32 },
    /// Explicit per-index intervals; falls back to `interval_minutes`
    /// past the end of the slice.
    Custom { intervals: Vec<u32> },
}

impl SnoozePolicy {
    /// Construct a policy with constant interval (no escalation).
    pub fn constant(max_count: u32, interval_minutes: u32) -> Self {
        Self {
            max_count,
            interval_minutes,
            escalation: None,
        }
    }

    /// True iff a snooze is permitted given the current count.
    pub fn can_snooze(&self, current_count: u32) -> bool {
        current_count < self.max_count
    }

    /// Interval (in minutes) the `snooze_index`-th snooze should use.
    /// Returns `None` once the snooze cap is reached.
    ///
    /// `snooze_index` is the 0-based index of the snooze about to occur:
    /// 0 = first snooze, 1 = second, etc. The persisted `snooze_count`
    /// after a successful snooze equals `snooze_index + 1`.
    pub fn interval_for(&self, snooze_index: u32) -> Option<u32> {
        if snooze_index >= self.max_count {
            return None;
        }
        let minutes = match &self.escalation {
            None => self.interval_minutes,
            Some(EscalationPolicy::Linear { step_minutes }) => self
                .interval_minutes
                .saturating_add(snooze_index.saturating_mul(*step_minutes)),
            Some(EscalationPolicy::Exponential { factor }) => {
                let scaled = (self.interval_minutes as f32) * factor.powi(snooze_index as i32);
                if scaled.is_finite() && scaled >= 0.0 {
                    scaled as u32
                } else {
                    self.interval_minutes
                }
            }
            Some(EscalationPolicy::Custom { intervals }) => intervals
                .get(snooze_index as usize)
                .copied()
                .unwrap_or(self.interval_minutes),
        };
        Some(minutes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_returns_same_interval_each_snooze() {
        let p = SnoozePolicy::constant(3, 5);
        assert_eq!(p.interval_for(0), Some(5));
        assert_eq!(p.interval_for(1), Some(5));
        assert_eq!(p.interval_for(2), Some(5));
        assert_eq!(p.interval_for(3), None);
    }

    #[test]
    fn can_snooze_returns_false_at_limit() {
        let p = SnoozePolicy::constant(3, 5);
        assert!(p.can_snooze(0));
        assert!(p.can_snooze(2));
        assert!(!p.can_snooze(3));
        assert!(!p.can_snooze(4));
    }

    #[test]
    fn max_count_zero_disables_snooze() {
        let p = SnoozePolicy::constant(0, 5);
        assert!(!p.can_snooze(0));
        assert_eq!(p.interval_for(0), None);
    }

    #[test]
    fn linear_escalation_adds_per_step() {
        let p = SnoozePolicy {
            max_count: 4,
            interval_minutes: 5,
            escalation: Some(EscalationPolicy::Linear { step_minutes: 2 }),
        };
        assert_eq!(p.interval_for(0), Some(5));
        assert_eq!(p.interval_for(1), Some(7));
        assert_eq!(p.interval_for(2), Some(9));
        assert_eq!(p.interval_for(3), Some(11));
        assert_eq!(p.interval_for(4), None);
    }

    #[test]
    fn exponential_escalation_multiplies_by_factor() {
        let p = SnoozePolicy {
            max_count: 4,
            interval_minutes: 5,
            escalation: Some(EscalationPolicy::Exponential { factor: 2.0 }),
        };
        assert_eq!(p.interval_for(0), Some(5));
        assert_eq!(p.interval_for(1), Some(10));
        assert_eq!(p.interval_for(2), Some(20));
        assert_eq!(p.interval_for(3), Some(40));
    }

    #[test]
    fn exponential_handles_nan_and_negative_factor_gracefully() {
        let p = SnoozePolicy {
            max_count: 4,
            interval_minutes: 5,
            escalation: Some(EscalationPolicy::Exponential { factor: f32::NAN }),
        };
        // NaN propagation produces non-finite values; we fall back to base.
        assert_eq!(p.interval_for(1), Some(5));
    }

    #[test]
    fn custom_escalation_uses_explicit_intervals() {
        let p = SnoozePolicy {
            max_count: 5,
            interval_minutes: 5,
            escalation: Some(EscalationPolicy::Custom {
                intervals: vec![1, 3, 7, 15],
            }),
        };
        assert_eq!(p.interval_for(0), Some(1));
        assert_eq!(p.interval_for(1), Some(3));
        assert_eq!(p.interval_for(2), Some(7));
        assert_eq!(p.interval_for(3), Some(15));
        // Past the explicit list: fall back to base interval.
        assert_eq!(p.interval_for(4), Some(5));
        // Past max_count: None.
        assert_eq!(p.interval_for(5), None);
    }

    #[test]
    fn linear_step_zero_is_constant() {
        let p = SnoozePolicy {
            max_count: 3,
            interval_minutes: 5,
            escalation: Some(EscalationPolicy::Linear { step_minutes: 0 }),
        };
        for i in 0..3 {
            assert_eq!(p.interval_for(i), Some(5));
        }
    }

    #[test]
    fn linear_saturates_on_overflow() {
        let p = SnoozePolicy {
            max_count: 5,
            interval_minutes: u32::MAX - 1,
            escalation: Some(EscalationPolicy::Linear { step_minutes: 1000 }),
        };
        // Saturating add prevents wrap.
        assert_eq!(p.interval_for(1), Some(u32::MAX));
    }

    #[test]
    fn property_can_snooze_implies_some_interval() {
        let p = SnoozePolicy::constant(10, 3);
        for i in 0..15 {
            assert_eq!(p.can_snooze(i), p.interval_for(i).is_some());
        }
    }
}
