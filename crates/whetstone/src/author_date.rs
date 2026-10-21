//! When a commit was authored, worded for the commit list: relative while
//! recent, then a short date in the commit's own time zone, as `git log`
//! shows its author date.

use std::time::{SystemTime, UNIX_EPOCH};

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
/// How long ago a date is still yesterday: up to two days.
const YESTERDAY: i64 = 2 * DAY;
/// How long ago a date is worded relative to now, rather than as a date:
/// up to six days.
const RELATIVE: i64 = 7 * DAY;

/// A commit's author date: when it was authored, with its author's UTC
/// offset, as the commit records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuthorDate(gix::date::Time);

/// How long a date's wording is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wording {
    /// In words: "2 hours ago", "yesterday", "Oct 14, 2025".
    Full,
    /// As short as it can be read, for a narrow column: "2h", "1d",
    /// "Oct 2025".
    Short,
}

impl AuthorDate {
    /// The author date `time` records.
    pub(crate) fn new(time: gix::date::Time) -> Self {
        Self(time)
    }

    /// The date worded as of `now`, as `wording` says: "just now" under a
    /// minute ago, then in minutes, hours, "yesterday" from one day ago and
    /// days up to six, and from a week ago its month and day in the
    /// commit's own time zone, with the year when it isn't the current one
    /// there. A date after now, from a clock that ran ahead, is a date too.
    pub(crate) fn worded(self, now: SystemTime, wording: Wording) -> String {
        let now = seconds_since_epoch(now);
        let ago = now.saturating_sub(self.0.seconds);
        let (count, unit, short_unit) = match (ago, wording) {
            (..0 | RELATIVE.., _) => return self.calendar_date(now, wording),
            (0..MINUTE, Wording::Full) => return "just now".to_owned(),
            (0..MINUTE, Wording::Short) => return "now".to_owned(),
            (MINUTE..HOUR, _) => (ago / MINUTE, "minute", 'm'),
            (HOUR..DAY, _) => (ago / HOUR, "hour", 'h'),
            (DAY..YESTERDAY, Wording::Full) => return "yesterday".to_owned(),
            _ => (ago / DAY, "day", 'd'),
        };
        match wording {
            Wording::Full if count == 1 => format!("1 {unit} ago"),
            Wording::Full => format!("{count} {unit}s ago"),
            Wording::Short => format!("{count}{short_unit}"),
        }
    }

    /// The date as its month and day in the commit's own time zone, with
    /// the year when it isn't the year it is there at `now`, in seconds
    /// since the Unix epoch. An offset that can't be a time zone's falls
    /// back to UTC, and a time no calendar holds to its seconds, as
    /// `git log --date=unix` shows it.
    fn calendar_date(self, now: i64, wording: Wording) -> String {
        let offset = if self.0.to_zoned().is_ok() {
            self.0.offset
        } else {
            0
        };
        let in_zone = |seconds| gix::date::Time { seconds, offset }.to_zoned();
        let Ok(date) = in_zone(self.0.seconds) else {
            return self.0.seconds.to_string();
        };
        let this_year = in_zone(now).is_ok_and(|now| now.year() == date.year());
        let format = match (this_year, wording) {
            (true, _) => "%b %-d",
            (false, Wording::Full) => "%b %-d, %Y",
            (false, Wording::Short) => "%b %Y",
        };
        date.strftime(format).to_string()
    }
}

/// `at` in seconds since the Unix epoch, negative before it.
fn seconds_since_epoch(at: SystemTime) -> i64 {
    match at.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            i64::try_from(before.duration().as_secs()).map_or(i64::MIN, i64::wrapping_neg)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    /// 2026-10-21 10:15:02 UTC.
    const NOW: i64 = 1_792_577_702;

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(NOW.unsigned_abs())
    }

    /// The date `ago` seconds before [`NOW`], in UTC, worded both ways.
    fn worded_ago(ago: i64) -> (String, String) {
        worded_at(NOW - ago, 0)
    }

    /// The date at `seconds` since the Unix epoch, `offset` seconds east of
    /// UTC, worded both ways.
    fn worded_at(seconds: i64, offset: i32) -> (String, String) {
        let date = AuthorDate::new(gix::date::Time { seconds, offset });
        (
            date.worded(now(), Wording::Full),
            date.worded(now(), Wording::Short),
        )
    }

    fn pair(full: &str, short: &str) -> (String, String) {
        (full.to_owned(), short.to_owned())
    }

    #[test]
    fn under_a_minute_ago_is_just_now() {
        assert_eq!(worded_ago(0), pair("just now", "now"));
        assert_eq!(worded_ago(MINUTE - 1), pair("just now", "now"));
    }

    #[test]
    fn under_an_hour_ago_is_in_minutes() {
        assert_eq!(worded_ago(MINUTE), pair("1 minute ago", "1m"));
        assert_eq!(worded_ago(5 * MINUTE + 30), pair("5 minutes ago", "5m"));
        assert_eq!(worded_ago(HOUR - 1), pair("59 minutes ago", "59m"));
    }

    #[test]
    fn under_a_day_ago_is_in_hours() {
        assert_eq!(worded_ago(HOUR), pair("1 hour ago", "1h"));
        assert_eq!(
            worded_ago(2 * HOUR + 59 * MINUTE),
            pair("2 hours ago", "2h")
        );
        assert_eq!(worded_ago(DAY - 1), pair("23 hours ago", "23h"));
    }

    #[test]
    fn one_day_ago_is_yesterday() {
        assert_eq!(worded_ago(DAY), pair("yesterday", "1d"));
        assert_eq!(worded_ago(2 * DAY - 1), pair("yesterday", "1d"));
    }

    #[test]
    fn up_to_six_days_ago_is_in_days() {
        assert_eq!(worded_ago(2 * DAY), pair("2 days ago", "2d"));
        assert_eq!(worded_ago(7 * DAY - 1), pair("6 days ago", "6d"));
    }

    #[test]
    fn a_week_ago_or_more_this_year_is_its_month_and_day() {
        // 2026-10-14 10:15:02 UTC.
        assert_eq!(worded_ago(7 * DAY), pair("Oct 14", "Oct 14"));
        // 2026-01-01 00:00:00 UTC.
        assert_eq!(worded_at(1_767_225_600, 0), pair("Jan 1", "Jan 1"));
    }

    #[test]
    fn a_date_in_another_year_says_the_year() {
        // 2025-10-14 10:15:02 UTC.
        assert_eq!(worded_ago(372 * DAY), pair("Oct 14, 2025", "Oct 2025"));
        // 2025-12-31 23:59:59 UTC.
        assert_eq!(
            worded_at(1_767_225_599, 0),
            pair("Dec 31, 2025", "Dec 2025")
        );
    }

    #[test]
    fn the_date_and_its_year_are_the_commits_own_time_zones() {
        // 2026-10-13 23:30 UTC is already 14 October two hours east.
        assert_eq!(
            worded_at(1_791_934_200, 2 * 3_600),
            pair("Oct 14", "Oct 14")
        );
        // 2025-12-31 23:30 UTC is already 2026 an hour east, and it is
        // 2026 there now too.
        assert_eq!(worded_at(1_767_223_800, 3_600), pair("Jan 1", "Jan 1"));
        // 2026-01-01 00:30 UTC is still 2025 five hours west.
        assert_eq!(
            worded_at(1_767_227_400, -5 * 3_600),
            pair("Dec 31, 2025", "Dec 2025")
        );
    }

    #[test]
    fn a_date_after_now_is_its_month_and_day() {
        // A clock that ran ahead: 2026-10-22 10:15:02 UTC.
        assert_eq!(worded_ago(-DAY), pair("Oct 22", "Oct 22"));
    }
}
