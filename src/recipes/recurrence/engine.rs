//! Recurrence next-fire computation.
//!
//! Given a [`Schedule`], the user's IANA time zone, and the current UTC
//! instant, returns the next UTC instant at which the schedule should
//! fire — strictly after `now`. Pure function: same inputs always yield
//! the same output (verified by property test).

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use thiserror::Error;

use super::dst::resolve_in_zone;
use super::schedule::Schedule;

/// Errors returned by [`next_fire`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RecurrenceError {
    /// The schedule has no enabled days (e.g. empty WeekdaySet).
    #[error("schedule has no enabled fire times")]
    NoFireTime,
    /// Cron expressions are reserved — not yet supported by the engine.
    #[error("cron schedules are not yet supported")]
    CronUnsupported,
    /// A calendar arithmetic edge case produced no valid date (e.g. day
    /// 31 in February with no clamp). Should be unreachable for the
    /// supported variants.
    #[error("internal date arithmetic failure")]
    InternalDateFailure,
}

/// Compute the next-fire UTC instant strictly after `now` for `schedule`
/// in `zone`.
pub fn next_fire(
    schedule: &Schedule,
    zone: &Tz,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, RecurrenceError> {
    match schedule {
        Schedule::OneTime { date, time } => {
            let local = date.and_time(*time);
            let instant = resolve_in_zone(local, zone).instant();
            if instant > now {
                Ok(instant)
            } else {
                // OneTime in the past — Recipe-level boot recovery decides
                // whether to fire-immediately or skip; the engine simply
                // signals "no future fire."
                Err(RecurrenceError::NoFireTime)
            }
        }
        Schedule::Weekdays { days, time } => {
            if days.is_empty() {
                return Err(RecurrenceError::NoFireTime);
            }
            // Walk forward day by day from today (in zone) until we find a
            // selected weekday whose local fire time is strictly after now.
            let now_local = now.with_timezone(zone);
            let today = now_local.date_naive();
            for offset in 0..=7_i64 {
                let candidate_date = today + Duration::days(offset);
                let weekday = candidate_date.weekday();
                if !days.contains(weekday) {
                    continue;
                }
                let local = candidate_date.and_time(*time);
                let candidate = resolve_in_zone(local, zone).instant();
                if candidate > now {
                    return Ok(candidate);
                }
            }
            // Should be unreachable — at most 7 days ahead must hit a
            // selected weekday with a future time.
            Err(RecurrenceError::InternalDateFailure)
        }
        Schedule::Monthly { day_of_month, time } => {
            let now_local = now.with_timezone(zone);
            let mut year = now_local.year();
            let mut month = now_local.month() as i32;
            // Try this month, next month, ... up to 12 months ahead.
            for _ in 0..=12 {
                let date = resolve_monthly_day(year, month as u32, *day_of_month)?;
                let local = date.and_time(*time);
                let candidate = resolve_in_zone(local, zone).instant();
                if candidate > now {
                    return Ok(candidate);
                }
                month += 1;
                if month > 12 {
                    month = 1;
                    year += 1;
                }
            }
            Err(RecurrenceError::InternalDateFailure)
        }
        Schedule::Cron { .. } => Err(RecurrenceError::CronUnsupported),
    }
}

/// Resolve a (year, month, day_of_month) tuple where `day_of_month == 0`
/// means "last day of month" and any value > days-in-month is clamped down.
fn resolve_monthly_day(
    year: i32,
    month: u32,
    day_of_month: u8,
) -> Result<NaiveDate, RecurrenceError> {
    let last_day = days_in_month(year, month);
    let target = if day_of_month == 0 {
        last_day
    } else {
        std::cmp::min(day_of_month as u32, last_day)
    };
    NaiveDate::from_ymd_opt(year, month, target).ok_or(RecurrenceError::InternalDateFailure)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    // Build first of next month, subtract one day.
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(next_year, next_month, 1).unwrap();
    (first_next - chrono::Days::new(1)).day()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::recurrence::schedule::WeekdaySet;
    use chrono::{NaiveTime, TimeZone, Weekday};

    fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
    }

    fn ny() -> Tz {
        chrono_tz::America::New_York
    }

    fn utc_zone() -> Tz {
        chrono_tz::UTC
    }

    #[test]
    fn one_time_in_future_returns_that_instant() {
        let schedule = Schedule::OneTime {
            date: NaiveDate::from_ymd_opt(2030, 6, 1).unwrap(),
            time: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
        };
        let now = utc(2026, 1, 1, 0, 0);
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        assert_eq!(next, utc(2030, 6, 1, 12, 0));
    }

    #[test]
    fn one_time_in_past_signals_no_fire() {
        let schedule = Schedule::OneTime {
            date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            time: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        };
        let now = utc(2026, 1, 1, 0, 0);
        assert_eq!(
            next_fire(&schedule, &utc_zone(), now),
            Err(RecurrenceError::NoFireTime)
        );
    }

    #[test]
    fn weekdays_picks_today_if_future() {
        // 2026-06-08 is a Monday. Schedule fires Mon 09:00.
        let schedule = Schedule::Weekdays {
            days: WeekdaySet::from_weekdays([Weekday::Mon]),
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        let now = utc(2026, 6, 8, 8, 0); // Mon 08:00 UTC
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        assert_eq!(next, utc(2026, 6, 8, 9, 0));
    }

    #[test]
    fn weekdays_skips_to_next_week_if_today_already_passed() {
        // 2026-06-08 is a Monday. It's now 18:00, schedule is 09:00.
        let schedule = Schedule::Weekdays {
            days: WeekdaySet::from_weekdays([Weekday::Mon]),
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        let now = utc(2026, 6, 8, 18, 0);
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        // Next Monday is 2026-06-15.
        assert_eq!(next, utc(2026, 6, 15, 9, 0));
    }

    #[test]
    fn weekdays_empty_set_errors() {
        let schedule = Schedule::Weekdays {
            days: WeekdaySet::NONE,
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        assert_eq!(
            next_fire(&schedule, &utc_zone(), utc(2026, 1, 1, 0, 0)),
            Err(RecurrenceError::NoFireTime)
        );
    }

    #[test]
    fn weekdays_finds_first_selected_day_within_week() {
        // Schedule fires Wed and Fri at 09:00. Today is Mon 2026-06-08.
        let schedule = Schedule::Weekdays {
            days: WeekdaySet::from_weekdays([Weekday::Wed, Weekday::Fri]),
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        let now = utc(2026, 6, 8, 18, 0);
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        // Wed is 2026-06-10.
        assert_eq!(next, utc(2026, 6, 10, 9, 0));
    }

    #[test]
    fn monthly_picks_this_month_if_future() {
        let schedule = Schedule::Monthly {
            day_of_month: 15,
            time: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        };
        let now = utc(2026, 6, 1, 0, 0);
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        assert_eq!(next, utc(2026, 6, 15, 8, 0));
    }

    #[test]
    fn monthly_skips_to_next_month_if_passed() {
        let schedule = Schedule::Monthly {
            day_of_month: 15,
            time: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        };
        let now = utc(2026, 6, 16, 0, 0);
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        assert_eq!(next, utc(2026, 7, 15, 8, 0));
    }

    #[test]
    fn monthly_last_day_clamps_to_february() {
        let schedule = Schedule::Monthly {
            day_of_month: 31,
            time: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        };
        // 2026-02 has 28 days.
        let now = utc(2026, 2, 1, 0, 0);
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        assert_eq!(next, utc(2026, 2, 28, 8, 0));
    }

    #[test]
    fn monthly_zero_means_last_day_of_month() {
        let schedule = Schedule::Monthly {
            day_of_month: 0,
            time: NaiveTime::from_hms_opt(23, 59, 0).unwrap(),
        };
        let now = utc(2026, 2, 1, 0, 0);
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        assert_eq!(next, utc(2026, 2, 28, 23, 59));
    }

    #[test]
    fn monthly_handles_year_boundary() {
        let schedule = Schedule::Monthly {
            day_of_month: 5,
            time: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        };
        let now = utc(2026, 12, 6, 0, 0);
        let next = next_fire(&schedule, &utc_zone(), now).unwrap();
        assert_eq!(next, utc(2027, 1, 5, 0, 0));
    }

    #[test]
    fn cron_returns_unsupported() {
        let schedule = Schedule::Cron {
            expression: "0 9 * * *".into(),
        };
        assert_eq!(
            next_fire(&schedule, &utc_zone(), utc(2026, 1, 1, 0, 0)),
            Err(RecurrenceError::CronUnsupported)
        );
    }

    #[test]
    fn weekday_in_dst_zone_is_correct_local_time() {
        // 2026-06-08 is a Monday. NY is in EDT (UTC-4) in June.
        // Schedule: Mon 09:00 local => 13:00 UTC.
        let schedule = Schedule::Weekdays {
            days: WeekdaySet::from_weekdays([Weekday::Mon]),
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        let now = utc(2026, 6, 8, 0, 0);
        let next = next_fire(&schedule, &ny(), now).unwrap();
        assert_eq!(next, utc(2026, 6, 8, 13, 0));
    }

    #[test]
    fn deterministic_for_same_input() {
        let schedule = Schedule::Weekdays {
            days: WeekdaySet::WEEKDAYS,
            time: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
        };
        let now = utc(2026, 6, 8, 12, 0);
        let a = next_fire(&schedule, &ny(), now).unwrap();
        let b = next_fire(&schedule, &ny(), now).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn weekday_schedule_advances_strictly_past_now() {
        // Schedule fires Mon 09:00 local. Now is exactly Mon 09:00 local.
        let schedule = Schedule::Weekdays {
            days: WeekdaySet::from_weekdays([Weekday::Mon]),
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        // Mon 09:00 in NY = 13:00 UTC.
        let now = utc(2026, 6, 8, 13, 0);
        let next = next_fire(&schedule, &ny(), now).unwrap();
        // Must be next Monday.
        assert_eq!(next, utc(2026, 6, 15, 13, 0));
    }
}
