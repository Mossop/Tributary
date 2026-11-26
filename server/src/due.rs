//! Default due dates, counted from a time each item's source reports.

use chrono::{DateTime, Datelike, Duration, Utc, Weekday};
use serde::{Deserialize, Serialize};

/// A source's default due date: `days` after each item's `due_from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultDue {
    pub days: u32,
    /// Count only Monday to Friday.
    pub business_days: bool,
}

impl DefaultDue {
    pub fn from(&self, from: DateTime<Utc>) -> DateTime<Utc> {
        add_days(from, self.days, self.business_days)
    }
}

fn is_weekend(time: DateTime<Utc>) -> bool {
    matches!(time.weekday(), Weekday::Sat | Weekday::Sun)
}

/// `days` days after `from`, at the same time of day. Business days step
/// forward a day at a time, counting only weekdays (in UTC, with no
/// holidays).
pub fn add_days(from: DateTime<Utc>, days: u32, business_days: bool) -> DateTime<Utc> {
    if !business_days {
        return from + Duration::days(i64::from(days));
    }
    let mut due = from;
    let mut left = days;
    while left > 0 {
        due += Duration::days(1);
        if !is_weekend(due) {
            left -= 1;
        }
    }
    due
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    /// A time in November 2026, when the 23rd is a Monday.
    fn day(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, hour, 30, 0).unwrap()
    }

    #[test]
    fn adds_calendar_days() {
        assert_eq!(day(23, 0).weekday(), Weekday::Mon);
        assert_eq!(add_days(day(27, 10), 3, false), day(30, 10));
        assert_eq!(add_days(day(27, 10), 0, false), day(27, 10));
    }

    #[test]
    fn adds_business_days() {
        // Wednesday + 1 is Thursday.
        assert_eq!(add_days(day(25, 9), 1, true), day(26, 9));
        // Friday + 1 is Monday.
        assert_eq!(add_days(day(27, 9), 1, true), day(30, 9));
        // Thursday + 3 skips the weekend.
        assert_eq!(
            add_days(day(26, 9), 3, true),
            Utc.with_ymd_and_hms(2026, 12, 1, 9, 30, 0).unwrap()
        );
        // Wednesday + 5 is the next Wednesday.
        assert_eq!(
            add_days(day(25, 9), 5, true),
            Utc.with_ymd_and_hms(2026, 12, 2, 9, 30, 0).unwrap()
        );
    }

    #[test]
    fn starts_counting_after_weekends() {
        // Saturday or Sunday + 1 is Monday.
        assert_eq!(add_days(day(28, 14), 1, true), day(30, 14));
        assert_eq!(add_days(day(29, 14), 1, true), day(30, 14));
        // Saturday + 2 is Tuesday.
        assert_eq!(
            add_days(day(28, 14), 2, true),
            Utc.with_ymd_and_hms(2026, 12, 1, 14, 30, 0).unwrap()
        );
    }
}
