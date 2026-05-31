//! Strongly-typed schedule values used by the Recurrence Engine.
//!
//! [`Schedule`] is the in-Rust working type; the on-disk persisted form is
//! [`crate::recipes::context::schema::ScheduleSpec`]. Conversion helpers handle the
//! round-trip — the disk form prefers strings/u8s for forward compatibility
//! while the in-memory form uses chrono types for arithmetic.

use chrono::{NaiveDate, NaiveTime, Weekday};

use crate::recipes::context::schema::ScheduleSpec;

/// Bitset of selected weekdays. Bit 0 = Monday, …, bit 6 = Sunday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WeekdaySet(pub u8);

impl WeekdaySet {
    pub const NONE: WeekdaySet = WeekdaySet(0);
    pub const ALL: WeekdaySet = WeekdaySet(0b0111_1111);
    pub const WEEKDAYS: WeekdaySet = WeekdaySet(0b0001_1111); // Mon-Fri
    pub const WEEKENDS: WeekdaySet = WeekdaySet(0b0110_0000); // Sat+Sun

    /// Construct from an iterator of weekdays.
    pub fn from_weekdays<I: IntoIterator<Item = Weekday>>(iter: I) -> Self {
        let mut set = Self::NONE;
        for w in iter {
            set.insert(w);
        }
        set
    }

    /// True if no day is selected.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True if `day` is selected.
    pub fn contains(self, day: Weekday) -> bool {
        self.0 & weekday_bit(day) != 0
    }

    /// Mark `day` as selected.
    pub fn insert(&mut self, day: Weekday) {
        self.0 |= weekday_bit(day);
    }

    /// Iterate selected weekdays in Mon..=Sun order.
    pub fn iter(self) -> impl Iterator<Item = Weekday> {
        const ORDER: [Weekday; 7] = [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ];
        ORDER.into_iter().filter(move |w| self.contains(*w))
    }
}

fn weekday_bit(d: Weekday) -> u8 {
    match d {
        Weekday::Mon => 1 << 0,
        Weekday::Tue => 1 << 1,
        Weekday::Wed => 1 << 2,
        Weekday::Thu => 1 << 3,
        Weekday::Fri => 1 << 4,
        Weekday::Sat => 1 << 5,
        Weekday::Sun => 1 << 6,
    }
}

/// In-memory schedule used by the Recurrence Engine.
///
/// Convert to/from the persisted [`ScheduleSpec`] via `try_from` /
/// `try_into`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    /// Single fire at this date and local time.
    OneTime { date: NaiveDate, time: NaiveTime },
    /// Recurring fires at `time` on each selected `days` weekday.
    Weekdays { days: WeekdaySet, time: NaiveTime },
    /// Recurring on a specific day-of-month. `0` means last day of month.
    Monthly { day_of_month: u8, time: NaiveTime },
    /// Cron expression (5-field). Reserved — not yet implemented in the
    /// engine; consumers should use the structured variants above.
    Cron { expression: String },
}

/// Conversion errors when reading a [`ScheduleSpec`] from disk.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleParseError {
    #[error("invalid date '{0}', expected YYYY-MM-DD")]
    InvalidDate(String),
    #[error("invalid hour {0}, expected 0..=23")]
    InvalidHour(u8),
    #[error("invalid minute {0}, expected 0..=59")]
    InvalidMinute(u8),
    #[error("invalid day of month {0}, expected 0..=31 (0 = last)")]
    InvalidDayOfMonth(u8),
}

fn parse_time(hour: u8, minute: u8) -> Result<NaiveTime, ScheduleParseError> {
    if hour > 23 {
        return Err(ScheduleParseError::InvalidHour(hour));
    }
    if minute > 59 {
        return Err(ScheduleParseError::InvalidMinute(minute));
    }
    NaiveTime::from_hms_opt(hour as u32, minute as u32, 0)
        .ok_or(ScheduleParseError::InvalidHour(hour))
}

impl TryFrom<&ScheduleSpec> for Schedule {
    type Error = ScheduleParseError;

    fn try_from(spec: &ScheduleSpec) -> Result<Self, Self::Error> {
        Ok(match spec {
            ScheduleSpec::OneTime { date, hour, minute } => {
                let parsed_date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .map_err(|_| ScheduleParseError::InvalidDate(date.clone()))?;
                Schedule::OneTime {
                    date: parsed_date,
                    time: parse_time(*hour, *minute)?,
                }
            }
            ScheduleSpec::Weekdays {
                days_mask,
                hour,
                minute,
            } => Schedule::Weekdays {
                days: WeekdaySet(*days_mask & 0b0111_1111),
                time: parse_time(*hour, *minute)?,
            },
            ScheduleSpec::Monthly {
                day_of_month,
                hour,
                minute,
            } => {
                if *day_of_month > 31 {
                    return Err(ScheduleParseError::InvalidDayOfMonth(*day_of_month));
                }
                Schedule::Monthly {
                    day_of_month: *day_of_month,
                    time: parse_time(*hour, *minute)?,
                }
            }
            ScheduleSpec::Cron { expression } => Schedule::Cron {
                expression: expression.clone(),
            },
        })
    }
}

impl TryFrom<ScheduleSpec> for Schedule {
    type Error = ScheduleParseError;
    fn try_from(value: ScheduleSpec) -> Result<Self, Self::Error> {
        Self::try_from(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_set_round_trips() {
        let s = WeekdaySet::from_weekdays([Weekday::Mon, Weekday::Wed, Weekday::Fri]);
        assert!(s.contains(Weekday::Mon));
        assert!(!s.contains(Weekday::Tue));
        assert!(s.contains(Weekday::Wed));
        assert!(s.contains(Weekday::Fri));
        assert_eq!(s.iter().count(), 3);
    }

    #[test]
    fn weekday_set_constants() {
        assert!(WeekdaySet::WEEKDAYS.contains(Weekday::Mon));
        assert!(WeekdaySet::WEEKDAYS.contains(Weekday::Fri));
        assert!(!WeekdaySet::WEEKDAYS.contains(Weekday::Sat));
        assert!(WeekdaySet::WEEKENDS.contains(Weekday::Sat));
        assert!(WeekdaySet::WEEKENDS.contains(Weekday::Sun));
        assert!(!WeekdaySet::WEEKENDS.contains(Weekday::Mon));
        assert_eq!(WeekdaySet::ALL.iter().count(), 7);
        assert!(WeekdaySet::NONE.is_empty());
    }

    #[test]
    fn one_time_spec_parses_into_schedule() {
        let spec = ScheduleSpec::OneTime {
            date: "2026-06-01".into(),
            hour: 7,
            minute: 30,
        };
        let s = Schedule::try_from(&spec).unwrap();
        match s {
            Schedule::OneTime { date, time } => {
                assert_eq!(date, NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
                assert_eq!(time, NaiveTime::from_hms_opt(7, 30, 0).unwrap());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn invalid_hour_is_rejected() {
        let spec = ScheduleSpec::OneTime {
            date: "2026-06-01".into(),
            hour: 24,
            minute: 0,
        };
        assert!(matches!(
            Schedule::try_from(&spec),
            Err(ScheduleParseError::InvalidHour(24))
        ));
    }

    #[test]
    fn invalid_date_is_rejected() {
        let spec = ScheduleSpec::OneTime {
            date: "not-a-date".into(),
            hour: 0,
            minute: 0,
        };
        assert!(matches!(
            Schedule::try_from(&spec),
            Err(ScheduleParseError::InvalidDate(_))
        ));
    }

    #[test]
    fn weekdays_spec_masks_high_bit() {
        let spec = ScheduleSpec::Weekdays {
            days_mask: 0xFF,
            hour: 7,
            minute: 0,
        };
        let s = Schedule::try_from(&spec).unwrap();
        if let Schedule::Weekdays { days, .. } = s {
            // Top bit dropped — only 0..=6 are valid weekday bits.
            assert_eq!(days.0, 0b0111_1111);
        } else {
            panic!("wrong variant");
        }
    }
}
