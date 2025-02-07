//! Utilities for weekly-time related logic.

use crate::weekly::{MinuteInWeek, MAX_WEEKLY_MINS, MAX_WEEKLY_SECS};
use anyhow::{anyhow, bail, ensure, Result};
use chrono::{DateTime, TimeZone, Weekday};
use fn_error_context::context;
use std::convert::TryInto;
use std::time::Duration;

/// Convert `MinuteInWeek` to a week day and time.
pub(crate) fn weekly_minute_as_weekday_time(weekly_minute: MinuteInWeek) -> (Weekday, u8, u8) {
    assert!(weekly_minute < MAX_WEEKLY_MINS);
    let days_from_monday = weekly_minute / (60_u32).saturating_mul(24);
    let weekday = match days_from_monday {
        0 => Weekday::Mon,
        1 => Weekday::Tue,
        2 => Weekday::Wed,
        3 => Weekday::Thu,
        4 => Weekday::Fri,
        5 => Weekday::Sat,
        _ => Weekday::Sun,
    };
    let hour: u8 = (weekly_minute % (60_u32).saturating_mul(24) / 60)
        .try_into()
        .unwrap();
    let minute: u8 = (weekly_minute % 60).try_into().unwrap();

    (weekday, hour, minute)
}

/// Convert datetime to minutes since beginning of week.
pub(crate) fn datetime_as_weekly_minute(datetime: &DateTime<impl TimeZone>) -> MinuteInWeek {
    use chrono::{Datelike, Timelike};

    let weekday = datetime.weekday();
    // SAFETY: hour() always <= 23.
    let hour = datetime.hour() as u8;
    // SAFETY: minutes() always <= 59.
    let minute = datetime.minute() as u8;

    time_as_weekly_minute(weekday, hour, minute)
}

/// Convert a point in weekly-time to minutes since beginning of week.
pub(crate) fn time_as_weekly_minute(day: chrono::Weekday, hour: u8, minute: u8) -> MinuteInWeek {
    let hour_minutes = u32::from(hour.min(23)).saturating_mul(60);
    let day_minutes = day
        .num_days_from_monday()
        .saturating_mul(24)
        .saturating_mul(60);
    let weekly_minute = day_minutes
        .saturating_add(hour_minutes)
        .saturating_add(u32::from(minute.min(59)));

    assert!(weekly_minute < MAX_WEEKLY_MINS);
    weekly_minute
}

/// Check duration for a sane lower and upper bound (whole week).
pub(crate) fn check_duration(length: &Duration) -> Result<()> {
    if length.as_secs() > MAX_WEEKLY_SECS {
        bail!("length longer than a week")
    };
    if length.as_secs() == 0 {
        bail!("zero-length duration")
    };

    Ok(())
}

/// Parse a week day string (English names).
pub(crate) fn weekday_from_string(input: &str) -> Result<Weekday> {
    let day = match input.to_lowercase().as_str() {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tuesady" => Weekday::Tue,
        "wed" | "wednesday" => Weekday::Wed,
        "thu" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => bail!("unrecognized week day: {}", input),
    };

    Ok(day)
}

/// Parse a time string (in 24h format).
///
/// ## Example
///
/// ```rust
/// let morning = time_from_string("6:20").unwrap();
/// assert_eq!(morning.0, 6);
/// assert_eq!(morning.0, 20);
///
/// let afternoon = time_from_string("14:05").unwrap();
/// assert_eq!(morning.0, 14);
/// assert_eq!(morning.0, 5);
/// ```
#[context("failed to parse time string")]
pub(crate) fn time_from_string(input: &str) -> Result<(u8, u8)> {
    let fields: Vec<_> = input.split(':').collect();
    if fields.len() != 2 {
        bail!("unrecognized time value: {}", input);
    }

    let hour = fields[0]
        .parse()
        .map_err(|_| anyhow!("unrecognized time (hour) value: {}", input))?;

    let minute = fields[1]
        .parse()
        .map_err(|_| anyhow!("unrecognized time (minute) value: {}", input))?;

    ensure!(hour <= 23 && minute <= 59, "invalid time: {}", input);
    Ok((hour, minute))
}

/// Validate a timespan (in minutes) and return its duration.
#[cfg(test)]
pub(crate) fn check_minutes(minutes: u32) -> Result<Duration> {
    let secs = u64::from(minutes).saturating_mul(60);
    let length = Duration::from_secs(secs);
    check_duration(&length)?;
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_check_duration() {
        check_duration(&Duration::from_secs(u64::MIN)).unwrap_err();
        check_duration(&Duration::from_secs(u64::MAX)).unwrap_err();

        let length = Duration::from_secs(42 * 60);
        check_duration(&length).unwrap();
        assert_eq!(length.as_secs(), 42 * 60);

        let max = check_minutes(crate::weekly::MAX_WEEKLY_MINS).unwrap();
        assert_eq!(
            max.as_secs(),
            u64::from(crate::weekly::MAX_WEEKLY_MINS) * 60
        );
    }

    #[test]
    fn test_check_minutes() {
        check_minutes(u32::MIN).unwrap_err();
        check_minutes(u32::MAX).unwrap_err();

        let length = check_minutes(42).unwrap();
        assert_eq!(length.as_secs(), 42 * 60);

        let max = check_minutes(crate::weekly::MAX_WEEKLY_MINS).unwrap();
        assert_eq!(
            max.as_secs(),
            u64::from(crate::weekly::MAX_WEEKLY_MINS) * 60
        );
    }

    #[test]
    fn test_weekday_from_string() {
        let mon1 = weekday_from_string("Mon").unwrap();
        assert_eq!(mon1, Weekday::Mon);

        let mon1 = weekday_from_string("monday").unwrap();
        assert_eq!(mon1, Weekday::Mon);

        weekday_from_string("domenica").unwrap_err();
    }

    #[test]
    fn test_time_from_string() {
        let t1 = time_from_string("12:45").unwrap();
        assert_eq!(t1, (12, 45));

        let t2 = time_from_string("07:5").unwrap();
        assert_eq!(t2, (7, 5));

        time_from_string("0x0A:0o70").unwrap_err();
        time_from_string("-00:00").unwrap_err();
        time_from_string("25:00").unwrap_err();
        time_from_string("23:60").unwrap_err();
    }

    #[test]
    fn test_weekly_minute_as_weekday_time() {
        let t = (24 * 60) * 2 + 60 * 4 + 5;
        let weekday_time = weekly_minute_as_weekday_time(t);
        assert_eq!((Weekday::Wed, 4, 5), weekday_time);
        let t = 7;
        let weekday_time = weekly_minute_as_weekday_time(t);
        assert_eq!((Weekday::Mon, 0, 7), weekday_time);
        let t = (24 * 60) * 6 + 60 * 23 + 59;
        let weekday_time = weekly_minute_as_weekday_time(t);
        assert_eq!((Weekday::Sun, 23, 59), weekday_time);
    }

    proptest! {
        #[test]
        fn proptest_time_from_string(time in any::<String>()){
            time_from_string(&time).unwrap_or_default();
        }

        #[test]
        fn proptest_weekday_from_string(day in any::<String>()){
            weekday_from_string(&day).unwrap_or(Weekday::Sun);
        }

        #[test]
        fn proptest_check_duration(length in any::<Duration>()){
            let res = match check_duration(&length) {
                Ok(_) => length,
                Err(_) => Duration::from_secs(1),
            };
            prop_assert!(res.as_secs() > 0);
            prop_assert!((res.as_secs() / 60) < u64::from(MAX_WEEKLY_MINS));
        }

        #[test]
        fn proptest_time_as_weekly_minute(day in ..=6u8, hour: u8, minute: u8){
            use num_traits::cast::FromPrimitive;

            let weekday = Weekday::from_u8(day).unwrap_or(Weekday::Sun);
            let res = time_as_weekly_minute(weekday, hour, minute);
            prop_assert!(res < MAX_WEEKLY_MINS);
        }
    }
}
