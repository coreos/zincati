//! Calendar-windows for events recurring on weekly basis.
//!
//! This contains helper logic to handle intervals of time which recur every week:
//!  * `WeeklyWindow`: a continuous interval over a single week.
//!  * `WeeklyCalendar`: a set of intervals on a weekly calendar.

// TODO(lucab): stabilize and split this to its own `weekly` crate.

pub(crate) mod utils;

use chrono::{DateTime, Utc};
use failure::Fallible;
use intervaltree::{Element, IntervalTree};
use serde::{Serialize, Serializer};
use std::cmp::Ordering;
use std::fmt::Write;
use std::iter::FromIterator;
use std::ops::Range;
use std::time::Duration;

/// Whole week duration, in minutes.
pub(crate) const MAX_WEEKLY_MINS: u32 = 7 * 24 * 60;

/// Whole week duration, in seconds.
pub(crate) const MAX_WEEKLY_SECS: u64 = (MAX_WEEKLY_MINS as u64) * 60;

/// A weekly point in time, as minutes since beginning of week (Monday 00:00).
pub(crate) type MinuteInWeek = u32;

/// Calendar for periodic time-windows, recurring on weekly basis.
#[derive(Clone, Debug)]
pub struct WeeklyCalendar {
    /// An immutable set of (possibly overlapping) intervals.
    windows: IntervalTree<MinuteInWeek, WeeklyWindow>,
}

impl WeeklyCalendar {
    /// Create a calendar from a vector of weekly windows.
    pub fn new(input: Vec<WeeklyWindow>) -> Self {
        let intervals = input
            .into_iter()
            .map(|win| Element::from((win.range_weekly_minutes(), win)));

        Self {
            windows: IntervalTree::from_iter(intervals),
        }
    }

    /// Return whether datetime is contained in this weekly calendar.
    pub fn contains_datetime(&self, datetime: &DateTime<Utc>) -> bool {
        let timepoint = utils::datetime_as_weekly_minute(datetime);
        self.windows.query_point(timepoint).count() > 0
    }

    /// Return the duration remaining till the next window containing the given datetime.
    ///
    /// This returns `None` if no windows are reachable.
    pub fn remaining_to_datetime(&self, datetime: &DateTime<Utc>) -> Option<chrono::Duration> {
        if self.is_empty() {
            return None;
        }

        // Already in a window, zero minutes.
        if self.contains_datetime(&datetime) {
            return Some(chrono::Duration::zero());
        }

        let timepoint = utils::datetime_as_weekly_minute(datetime);
        // Next window is this week, just subtract remaining minutes.
        if let Some(next) = self
            .windows
            .iter_sorted()
            .find(|x| x.range.start >= timepoint)
        {
            let remaining_mins = next.range.start.saturating_sub(timepoint);
            return Some(chrono::Duration::minutes(i64::from(remaining_mins)));
        };

        // Next window is not this week, wrap remaining minutes to the first
        // window of the next week (calendar has been already verified non-empty).
        let remaining_mins = {
            let remaining_this_week: i64 = MAX_WEEKLY_MINS.saturating_sub(timepoint).into();
            let first_window_next_week = self
                .windows
                .iter_sorted()
                .next()
                .expect("unexpected empty weekly calendar");
            remaining_this_week.saturating_add(first_window_next_week.range.start.into())
        };
        Some(chrono::Duration::minutes(remaining_mins))
    }

    /// Format remaining duration till the next window in human terms.
    pub fn human_remaining_duration(remaining: &chrono::Duration) -> Fallible<String> {
        if remaining.is_zero() {
            return Ok("now".to_string());
        }

        let mut human_readable = "in".to_string();
        let days = remaining.num_days() % 7;
        let earlier_output = if days > 0 {
            write!(&mut human_readable, " {}d", days)?;
            true
        } else {
            false
        };
        let hours = remaining.num_hours() % 24;
        if hours > 0 || earlier_output {
            write!(&mut human_readable, " {}h", hours)?;
        }
        let minutes = remaining.num_minutes() % 60;
        write!(&mut human_readable, " {}m", minutes)?;

        Ok(human_readable)
    }

    /// Return the measured length of the calendar, in minutes.
    ///
    /// In case of overlapping windows, measured length is the actual amount
    /// of weekly minutes in the calendar. Overlapped intervals are coalesced
    /// in order to avoid double-counting.
    #[allow(clippy::reversed_empty_ranges)]
    pub fn length_minutes(&self) -> u64 {
        let mut measured = 0u32;
        let mut last_range = Range {
            start: 0u32,
            end: 0u32,
        };

        for win in self.windows.iter_sorted() {
            if win.range.start > last_range.end {
                // Non-overlapping window, update accumulator and use this as last range.
                let last_length = last_range
                    .end
                    .saturating_sub(last_range.start)
                    .saturating_sub(1);
                measured = measured.saturating_add(last_length);
                last_range = win.range.clone();
            } else {
                // Overlapping window, coalesce into the last range.
                last_range.end = u32::max(last_range.end, win.range.end);
            };
        }
        // Account for the still pending length of the last range.
        let last_length = last_range
            .end
            .saturating_sub(last_range.start)
            .saturating_sub(1);
        measured = measured.saturating_add(last_length);

        u64::from(measured)
    }

    /// Return true if the calendar contains no time-windows.
    pub fn is_empty(&self) -> bool {
        self.windows.iter().next().is_none()
    }

    /// Return total length of all windows in the calendar, in minutes.
    ///
    /// In case of overlapping windows, total length can be larger than the
    /// actual amount of weekly minutes in the calendar.
    #[cfg(test)]
    pub fn total_length_minutes(&self) -> u64 {
        self.windows.iter().fold(0u64, |len, win| {
            len.saturating_add(win.value.length_minutes().into())
        })
    }

    /// Return all weekly windows (if any) which contain a given datetime.
    #[cfg(test)]
    pub fn containing_windows(&self, datetime: &DateTime<Utc>) -> Vec<&WeeklyWindow> {
        let timepoint = utils::datetime_as_weekly_minute(datetime);
        self.windows
            .query_point(timepoint)
            .map(|elem| &elem.value)
            .collect()
    }
}

impl Serialize for WeeklyCalendar {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;

        let len = self.windows.iter().count();
        let mut seq = serializer.serialize_seq(Some(len))?;
        for interval in self.windows.iter() {
            seq.serialize_element(&interval.value)?;
        }
        seq.end()
    }
}

impl Default for WeeklyCalendar {
    fn default() -> Self {
        Self::new(vec![])
    }
}

/// Timespan with a fixed duration, recurring on weekly basis.
///
/// Length duration is capped so that a window never crosses week boundary.
#[derive(Clone, Debug, Eq, Serialize)]
pub struct WeeklyWindow {
    start_day: chrono::Weekday,
    start_hour: u8,
    start_minute: u8,
    length: Duration,
}

impl WeeklyWindow {
    /// Parse a timespan into weekly windows.
    ///
    /// On success, this returns a non-empty vector with at most two weekly windows.
    pub fn parse_timespan(
        start_day: chrono::Weekday,
        start_hour: u8,
        start_minute: u8,
        length: Duration,
    ) -> Fallible<Vec<Self>> {
        // Sanity check inputs (start and length).
        failure::ensure!(
            start_hour <= 24 && start_minute <= 59,
            "invalid start time: {}:{}",
            start_hour,
            start_minute
        );
        utils::check_duration(&length)?;

        // Chop length at week boundary. Any seconds past Sunday end are carried into
        // remaining length.
        let remaining_len = {
            let start = utils::time_as_weekly_minute(start_day, start_hour, start_minute);
            let end_of_timespan_secs = u64::from(start)
                .saturating_mul(60)
                .saturating_add(length.as_secs());
            let remaining_secs = end_of_timespan_secs.saturating_sub(MAX_WEEKLY_SECS);
            Duration::from_secs(remaining_secs)
        };
        let chopped_len = length - remaining_len;

        // There is always at least one window for any non-zero timespan.
        utils::check_duration(&chopped_len)?;
        let win1 = Self {
            start_day,
            start_hour,
            start_minute,
            length: chopped_len,
        };
        let mut windows = vec![win1];

        // Remaining length (if any) is wrapped back to Monday start.
        if remaining_len.as_secs() > 0 {
            utils::check_duration(&remaining_len)?;
            let win2 = Self {
                start_day: chrono::Weekday::Mon,
                start_hour: 0,
                start_minute: 0,
                length: remaining_len,
            };
            windows.push(win2);
        }

        Ok(windows)
    }

    /// Return window length, in minutes.
    pub fn length_minutes(&self) -> u32 {
        // SAFETY: invariant `length < MAX_WEEKLY_MINS < u32::MAX`
        (self.length.as_secs() / 60) as u32
    }

    /// Return the weekly range covered by this window, in weekly minutes.
    pub fn range_weekly_minutes(&self) -> Range<MinuteInWeek> {
        // NOTE(lucab): Range in Rust does not include the upper limit, so
        // this accounts for a +1 on the end value.
        Range {
            start: self.start_minutes(),
            end: self.end_minutes().saturating_add(1),
        }
    }

    /// Window start, in minutes since beginning of week.
    fn start_minutes(&self) -> MinuteInWeek {
        let minutes = u32::from(self.start_minute);
        let hours = u32::from(self.start_hour).saturating_mul(60);
        let days = self
            .start_day
            .num_days_from_monday()
            .saturating_mul(24)
            .saturating_mul(60);
        days.saturating_add(hours).saturating_add(minutes)
    }

    /// Window end, in minutes since beginning of week.
    fn end_minutes(&self) -> MinuteInWeek {
        let start = self.start_minutes();
        let length = self.length_minutes();
        start.saturating_add(length)
    }

    /// Return whether datetime is contained in this window.
    #[cfg(test)]
    pub fn contains_datetime(&self, datetime: &DateTime<Utc>) -> bool {
        let instant = utils::datetime_as_weekly_minute(datetime);
        self.start_minutes() <= instant && instant <= self.end_minutes()
    }
}

impl Ord for WeeklyWindow {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.start_minutes().cmp(&other.start_minutes()) {
            Ordering::Equal => self.end_minutes().cmp(&other.end_minutes()),
            cmp => cmp,
        }
    }
}

impl PartialOrd for WeeklyWindow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for WeeklyWindow {
    fn eq(&self, other: &Self) -> bool {
        self.start_minutes() == other.start_minutes() && self.end_minutes() == other.end_minutes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_basic() {
        let start_minutes = (2 * 24 * 60) + (6 * 60);
        let end_minutes = start_minutes + 45;
        let length = utils::check_minutes(45).unwrap();
        let windows = WeeklyWindow::parse_timespan(chrono::Weekday::Wed, 6, 00, length).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].length_minutes(), 45);
        assert_eq!(windows[0].start_minutes(), start_minutes);
        assert_eq!(windows[0].end_minutes(), end_minutes);
    }

    #[test]
    fn window_split_timespan() {
        let length = utils::check_minutes(60).unwrap();
        let windows = WeeklyWindow::parse_timespan(chrono::Weekday::Sun, 23, 45, length).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].length_minutes(), 15);

        assert_eq!(windows[1].length_minutes(), 45);
        assert_eq!(windows[1].start_minutes(), 0);
        assert_eq!(windows[1].end_minutes(), 45);
    }

    #[test]
    fn calendar_basic() {
        let length = utils::check_minutes(60).unwrap();
        let windows = WeeklyWindow::parse_timespan(chrono::Weekday::Mon, 23, 45, length).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].length_minutes(), 60);

        let calendar = WeeklyCalendar::new(windows);
        assert_eq!(calendar.windows.iter().count(), 1);
    }

    #[test]
    fn window_contains_datetime() {
        let length = utils::check_minutes(120).unwrap();
        let windows = WeeklyWindow::parse_timespan(chrono::Weekday::Mon, 14, 30, length).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].length_minutes(), 120);

        let before_start = DateTime::parse_from_rfc3339("2019-06-24T14:29:59+00:00").unwrap();
        assert!(!windows[0].contains_datetime(&before_start.into()));

        let start = DateTime::parse_from_rfc3339("2019-06-24T14:30:00+00:00").unwrap();
        assert!(windows[0].contains_datetime(&start.into()));

        let after_start = DateTime::parse_from_rfc3339("2019-06-24T14:30:00+00:00").unwrap();
        assert!(windows[0].contains_datetime(&after_start.into()));

        let before_end = DateTime::parse_from_rfc3339("2019-06-24T16:29:59+00:00").unwrap();
        assert!(windows[0].contains_datetime(&before_end.into()));

        let end = DateTime::parse_from_rfc3339("2019-06-24T16:30:59+00:00").unwrap();
        assert!(windows[0].contains_datetime(&end.into()));

        let after_end = DateTime::parse_from_rfc3339("2019-06-24T16:31:00+00:00").unwrap();
        assert!(!windows[0].contains_datetime(&after_end.into()));
    }

    #[test]
    fn window_week_boundary() {
        let length = utils::check_minutes(1).unwrap();
        let single = WeeklyWindow::parse_timespan(chrono::Weekday::Sun, 23, 59, length).unwrap();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].length_minutes(), 1);

        let length = utils::check_minutes(2).unwrap();
        let chopped = WeeklyWindow::parse_timespan(chrono::Weekday::Sun, 23, 59, length).unwrap();
        assert_eq!(chopped.len(), 2);
        assert_eq!(chopped[0].length_minutes(), 1);
        assert_eq!(chopped[1].length_minutes(), 1);
    }

    #[test]
    fn calendar_contains_datetime() {
        let length = utils::check_minutes(75).unwrap();
        let windows = WeeklyWindow::parse_timespan(chrono::Weekday::Tue, 21, 0, length).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].length_minutes(), 75);

        let calendar = WeeklyCalendar::new(windows);
        assert_eq!(calendar.windows.iter().count(), 1);

        let datetime = DateTime::parse_from_rfc3339("2019-06-25T21:10:00+00:00").unwrap();
        assert!(calendar.contains_datetime(&datetime.into()));
    }

    #[test]
    fn calendar_whole_week() {
        let length = utils::check_minutes(MAX_WEEKLY_MINS).unwrap();
        let windows = WeeklyWindow::parse_timespan(chrono::Weekday::Mon, 0, 0, length).unwrap();
        assert_eq!(windows.len(), 1);

        let calendar = WeeklyCalendar::new(windows);
        assert_eq!(calendar.windows.iter().count(), 1);
        assert_eq!(calendar.total_length_minutes(), u64::from(MAX_WEEKLY_MINS));
        assert_eq!(calendar.length_minutes(), u64::from(MAX_WEEKLY_MINS));

        let datetime = chrono::Utc::now();
        assert!(calendar.contains_datetime(&datetime));
    }

    #[test]
    fn calendar_containing_window() {
        let length = utils::check_minutes(75).unwrap();
        let windows = WeeklyWindow::parse_timespan(chrono::Weekday::Tue, 21, 0, length).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].length_minutes(), 75);

        let calendar = WeeklyCalendar::new(windows.clone());
        assert_eq!(calendar.windows.iter().count(), 1);

        let datetime = DateTime::parse_from_rfc3339("2019-06-25T21:10:00+00:00").unwrap();
        assert!(calendar.contains_datetime(&datetime.into()));

        let containing_windows = calendar.containing_windows(&datetime.into());
        assert_eq!(containing_windows.len(), 1);
        assert_eq!(containing_windows[0], &windows[0]);
    }

    #[test]
    fn calendar_length() {
        let l1 = utils::check_minutes(45).unwrap();
        let mut w1 = WeeklyWindow::parse_timespan(chrono::Weekday::Mon, 1, 15, l1).unwrap();
        assert_eq!(w1.len(), 1);
        assert_eq!(w1[0].length_minutes(), 45);

        let l2 = utils::check_minutes(120).unwrap();
        let w2 = WeeklyWindow::parse_timespan(chrono::Weekday::Sun, 23, 30, l2).unwrap();
        assert_eq!(w2.len(), 2);
        assert_eq!(w2[0].length_minutes(), 30);
        assert_eq!(w2[1].length_minutes(), 90);

        w1.extend(w2);
        let calendar = WeeklyCalendar::new(w1);
        assert_eq!(calendar.windows.iter().count(), 3);

        assert_eq!(calendar.total_length_minutes(), 165);
        assert_eq!(calendar.length_minutes(), 150);
    }

    #[test]
    fn datetime_remaining() {
        let length = utils::check_minutes(15).unwrap();
        let w1 = WeeklyWindow::parse_timespan(chrono::Weekday::Mon, 1, 30, length).unwrap();
        let calendar = WeeklyCalendar::new(w1);

        let cases = vec![
            ("2020-11-23T00:15:00+00:00", 60 + 15),
            ("2020-11-23T01:29:30+00:00", 1),
            ("2020-11-23T01:30:00+00:00", 0),
            ("2020-11-23T01:45:00+00:00", 0),
            ("2020-11-23T02:00:00+00:00", 60 * 24 * 7 - 120 + 90),
            ("2020-11-22T01:30:00+00:00", 60 * 24),
        ];
        for (input, remaining) in cases {
            let datetime = DateTime::parse_from_rfc3339(input).unwrap();
            let output = calendar
                .remaining_to_datetime(&datetime.into())
                .unwrap()
                .num_minutes();
            assert_eq!(output, remaining, "{}", input);
        }
    }

    #[test]
    fn human_remaining() {
        use chrono::Duration;

        let cases = vec![
            (0, "now"),
            (1, "in 1m"),
            (59, "in 59m"),
            (60, "in 1h 0m"),
            (61, "in 1h 1m"),
            (120, "in 2h 0m"),
            (1439, "in 23h 59m"),
            (1440, "in 1d 0h 0m"),
            (1441, "in 1d 0h 1m"),
            (1501, "in 1d 1h 1m"),
            (2879, "in 1d 23h 59m"),
            (2880, "in 2d 0h 0m"),
            (4503, "in 3d 3h 3m"),
        ];

        for (mins, human) in cases {
            let remaining = Duration::minutes(mins);
            let output = WeeklyCalendar::human_remaining_duration(&remaining).unwrap();
            assert_eq!(output, human, "{}", mins);
        }
    }
}
