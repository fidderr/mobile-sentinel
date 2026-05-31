//! DST resolution rules for the Recurrence Engine.
//!
//! When a computed local time falls inside a Daylight-Saving-Time gap or
//! overlap, we follow these documented rules (see Requirement 8.3 / 8.4):
//!
//! - **Spring-forward gap** (the local time does not exist): advance to the
//!   first valid instant after the gap. The alarm fires slightly late but
//!   never skips.
//! - **Fall-back overlap** (the local time exists twice): fire at the
//!   *earlier* UTC occurrence. The alarm fires exactly once.
//!
//! Both rules are deterministic: same `(local_time, zone)` always
//! resolves to the same `DateTime<Utc>`.

use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

/// Outcome of resolving a local naive datetime in a specific zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Unique, valid resolution.
    Single(DateTime<Utc>),
    /// Spring-forward gap encountered; we advanced to `gap_end`.
    SpringForward {
        original: NaiveDateTime,
        gap_end: DateTime<Utc>,
    },
    /// Fall-back overlap encountered; we used the earlier occurrence.
    FallBack {
        original: NaiveDateTime,
        earlier: DateTime<Utc>,
        later: DateTime<Utc>,
    },
}

impl Resolution {
    /// The chosen UTC instant for this resolution.
    pub fn instant(&self) -> DateTime<Utc> {
        match self {
            Resolution::Single(dt)
            | Resolution::SpringForward { gap_end: dt, .. }
            | Resolution::FallBack { earlier: dt, .. } => *dt,
        }
    }
}

/// Resolve `local` into UTC under the given `zone`, applying the documented
/// DST rules.
pub fn resolve_in_zone(local: NaiveDateTime, zone: &Tz) -> Resolution {
    match zone.from_local_datetime(&local) {
        chrono::LocalResult::Single(dt) => Resolution::Single(dt.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(earliest, latest) => Resolution::FallBack {
            original: local,
            earlier: earliest.with_timezone(&Utc),
            later: latest.with_timezone(&Utc),
        },
        chrono::LocalResult::None => {
            // Spring-forward: walk forward minute-by-minute until a valid
            // local time exists. Two hours is safely beyond any documented
            // DST jump (max is 1h in IANA tzdata).
            for offset_min in 1..=180 {
                let candidate = local + Duration::minutes(offset_min);
                if let chrono::LocalResult::Single(dt) = zone.from_local_datetime(&candidate) {
                    return Resolution::SpringForward {
                        original: local,
                        gap_end: dt.with_timezone(&Utc),
                    };
                }
                if let chrono::LocalResult::Ambiguous(earliest, _) =
                    zone.from_local_datetime(&candidate)
                {
                    return Resolution::SpringForward {
                        original: local,
                        gap_end: earliest.with_timezone(&Utc),
                    };
                }
            }
            // Should be unreachable for any real IANA zone — pick a safe
            // deterministic fallback (treat as UTC) so we never panic.
            Resolution::SpringForward {
                original: local,
                gap_end: Utc.from_utc_datetime(&local),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    fn ny() -> Tz {
        chrono_tz::America::New_York
    }

    fn ldn() -> Tz {
        chrono_tz::Europe::London
    }

    fn mk(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
    }

    #[test]
    fn ordinary_time_resolves_singly() {
        // March 1 2026 in NY — far from any DST transition.
        let local = mk(2026, 3, 1, 12, 0);
        let r = resolve_in_zone(local, &ny());
        assert!(matches!(r, Resolution::Single(_)));
    }

    #[test]
    fn spring_forward_gap_advances_to_first_valid_instant() {
        // 2026-03-08 02:30 in America/New_York is inside the spring-forward
        // gap (clocks jump 02:00 -> 03:00 EST -> EDT).
        let local = mk(2026, 3, 8, 2, 30);
        let r = resolve_in_zone(local, &ny());
        match r {
            Resolution::SpringForward { gap_end, .. } => {
                // The first valid local time after 02:30 is 03:00 EDT,
                // which is 07:00 UTC.
                let expected = chrono::Utc
                    .with_ymd_and_hms(2026, 3, 8, 7, 0, 0)
                    .single()
                    .unwrap();
                assert_eq!(gap_end, expected);
            }
            other => panic!("expected SpringForward, got {other:?}"),
        }
    }

    #[test]
    fn fall_back_overlap_picks_earlier_instant() {
        // 2026-11-01 01:30 in America/New_York happens twice (EDT then EST).
        let local = mk(2026, 11, 1, 1, 30);
        let r = resolve_in_zone(local, &ny());
        match r {
            Resolution::FallBack { earlier, later, .. } => {
                // Earlier is 05:30 UTC (EDT), later is 06:30 UTC (EST).
                let earlier_expected = chrono::Utc
                    .with_ymd_and_hms(2026, 11, 1, 5, 30, 0)
                    .single()
                    .unwrap();
                let later_expected = chrono::Utc
                    .with_ymd_and_hms(2026, 11, 1, 6, 30, 0)
                    .single()
                    .unwrap();
                assert_eq!(earlier, earlier_expected);
                assert_eq!(later, later_expected);
                assert!(earlier < later);
            }
            other => panic!("expected FallBack, got {other:?}"),
        }
    }

    #[test]
    fn london_dst_transitions_are_correct() {
        // 2026-03-29 01:30 London (BST starts) — gap.
        let gap = resolve_in_zone(mk(2026, 3, 29, 1, 30), &ldn());
        assert!(matches!(gap, Resolution::SpringForward { .. }));
        // 2026-10-25 01:30 London (BST ends) — overlap.
        let overlap = resolve_in_zone(mk(2026, 10, 25, 1, 30), &ldn());
        assert!(matches!(overlap, Resolution::FallBack { .. }));
    }

    #[test]
    fn deterministic_for_same_input() {
        let local = mk(2026, 3, 8, 2, 30);
        let a = resolve_in_zone(local, &ny()).instant();
        let b = resolve_in_zone(local, &ny()).instant();
        assert_eq!(a, b);
    }
}
