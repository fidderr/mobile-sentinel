//! Recurrence Engine — DST-correct next-fire computation.
//!
//! Used by every Recipe that has a [`crate::recipes::context::schema::ScheduleSpec`].
//! Pure function: same `(now, schedule, zone)` always produces the same
//! `next_fire` (verified by property test). [`dst`] has the rules for daylight saving.

pub mod dst;
pub mod engine;
pub mod schedule;

pub use engine::{next_fire, RecurrenceError};
pub use schedule::{Schedule, WeekdaySet};
