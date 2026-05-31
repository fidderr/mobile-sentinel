//! Recurrence Engine — DST-correct next-fire computation.
//!
//! Used by every Recipe that has a [`crate::recipes::context::schema::ScheduleSpec`].
//! Pure function: same `(now, schedule, zone)` always produces the same
//! `next_fire` (verified by property test).
//!
//! See `requirements.md §Requirement 8` for the DST resolution rules and
//! `design.md §8 Recurrence Engine`.

pub mod dst;
pub mod engine;
pub mod schedule;

pub use engine::{next_fire, RecurrenceError};
pub use schedule::{Schedule, WeekdaySet};
