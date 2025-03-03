//! Strategy for periodic (weekly) updates.

use crate::config::inputs;
use crate::weekly::{utils, WeeklyCalendar, WeeklyWindow};
use anyhow::{Context, Error, Result};
use chrono::{TimeZone, Utc};
use fn_error_context::context;
use futures::future;
use futures::prelude::*;
use log::trace;
use serde::Serialize;
use std::fs::read_link;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;
use tzfile::Tz;

/// Strategy for periodic (weekly) updates.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StrategyPeriodic {
    /// Whitelisted time windows during which updates are allowed.
    schedule: WeeklyCalendar,
    /// Time zone in which time windows are defined in.
    #[serde(skip_serializing)]
    pub(crate) time_zone: Tz,
    /// Time zone name.
    tz_name: String,
}

impl Default for StrategyPeriodic {
    fn default() -> Self {
        let utc = "UTC";
        StrategyPeriodic {
            schedule: WeeklyCalendar::default(),
            time_zone: Tz::named(utc).unwrap(),
            tz_name: utc.to_string(),
        }
    }
}

impl StrategyPeriodic {
    /// Strategy label/name.
    pub const LABEL: &'static str = "periodic";

    /// Build a new periodic strategy.
    #[context("failed to parse periodic strategy")]
    pub fn new(cfg: inputs::UpdateInput) -> Result<Self> {
        let (time_zone, tz_name) = Self::get_time_zone_info_from_cfg(&cfg.periodic)?;

        let mut intervals = Vec::with_capacity(cfg.periodic.intervals.len());
        for entry in cfg.periodic.intervals {
            let weekday = utils::weekday_from_string(&entry.start_day)?;
            let start = utils::time_from_string(&entry.start_time)?;
            let length = Duration::from_secs(u64::from(entry.length_minutes).saturating_mul(60));
            let windows = WeeklyWindow::parse_timespan(weekday, start.0, start.1, length)?;
            intervals.extend(windows);
        }

        let calendar = WeeklyCalendar::new(intervals);
        match calendar.length_minutes() {
            0 => anyhow::bail!(
                "invalid or missing periodic updates configuration: weekly calendar length is zero"
            ),
            n => log::trace!("periodic updates, weekly calendar length: {} minutes", n),
        };

        let strategy = Self {
            schedule: calendar,
            time_zone,
            tz_name,
        };
        Ok(strategy)
    }

    /// Getter function for `StrategyPeriodic`'s `tz_name` field.
    pub fn tz_name(&self) -> &str {
        self.tz_name.as_str()
    }

    /// Get the time zone from `periodic` strategy config, returning a `Tz` and its name
    /// in a tuple.
    #[context("failed to get time zone info from config")]
    fn get_time_zone_info_from_cfg(cfg: &inputs::PeriodicInput) -> Result<(Tz, String)> {
        let tz;
        let tz_name;
        if &cfg.time_zone == "localtime" {
            let local_time_path = Path::new("/etc/localtime");
            // Use `read_link()` instead of `exists()` because we only want to check for
            // the existence of the `/etc/localtime` symlink, not whether it points to
            // a valid file (`read_link()` returns an error if symlink doesn't exist).
            if read_link(local_time_path).is_err() {
                let utc = "UTC";
                tz = Tz::named(utc)
                    .with_context(|| format!("failed to parse time zone named: {}", utc));
                tz_name = utc.to_string();
            } else {
                // Until `tzfile::Tz` has some way of getting its name or unique identifier, do
                // the parsing of `/etc/localtime` ourselves here so we can get a `tz_str` to cache.
                let tz_path = local_time_path.canonicalize()?;
                let tz_str = tz_path
                    .strip_prefix(Path::new("/usr/share/zoneinfo"))
                    .context(
                        "`/etc/localtime` does not link to a location in `/usr/share/zoneinfo`",
                    )?
                    .to_str()
                    .unwrap_or_default();
                tz = Tz::named(tz_str)
                    .with_context(|| format!("failed to parse time zone named: {}", tz_str));
                tz_name = tz_str.to_string();
            }
        } else {
            tz = Tz::named(&cfg.time_zone)
                .with_context(|| format!("failed to parse time zone named: {}", &cfg.time_zone));
            tz_name = cfg.time_zone.to_string();
        }

        tz.map(|tz| (tz, tz_name))
    }

    /// Return the measured length of the schedule, in minutes.
    pub(crate) fn schedule_length_minutes(&self) -> u64 {
        self.schedule.length_minutes()
    }

    /// Return the weekday and time of the next window, in human terms.
    pub(crate) fn human_next_window(&self) -> String {
        let naive_utc_dt = Utc::now().naive_utc();
        let dt = (&self.time_zone).from_utc_datetime(&naive_utc_dt);
        let next_window_minute_in_week = self.schedule.next_window_minute_in_week(&dt);

        match next_window_minute_in_week {
            Some(minute_in_week) => {
                let (weekday, hour, minute) = utils::weekly_minute_as_weekday_time(minute_in_week);
                format!(
                    "at {}:{} on {} ({}), subject to time zone caveats.",
                    hour, minute, weekday, self.tz_name
                )
            }
            None => "not found".to_string(),
        }
    }

    /// Return the remaining duration to next window, in human terms.
    pub(crate) fn human_remaining(&self) -> String {
        let datetime = chrono::Utc::now();
        let remaining = self.schedule.remaining_to_datetime(&datetime);
        match remaining {
            None => "not found".to_string(),
            Some(ref d) => WeeklyCalendar::human_remaining_duration(d)
                .unwrap_or_else(|_| "unknown".to_string()),
        }
    }

    /// Return some human-friendly information about `PeriodicStrategy`'s calendar.
    pub(crate) fn calendar_summary(&self) -> String {
        format!(
            "total schedule length {} minutes; next window {}",
            self.schedule_length_minutes(),
            if self.tz_name() != "UTC" || self.tz_name() != "Etc/UTC" {
                self.human_next_window()
            } else {
                // It is likely difficult for users to reason about UTC dates and times,
                // so display remaining time, instead.
                self.human_remaining()
            }
        )
    }

    /// Check if finalization is allowed.
    pub(crate) fn can_finalize(&self) -> Pin<Box<dyn Future<Output = Result<bool, Error>>>> {
        let naive_utc_dt = Utc::now().naive_utc();
        let dt = (&self.time_zone).from_utc_datetime(&naive_utc_dt);
        let allowed = self.schedule.contains_datetime(&dt);

        trace!("periodic strategy, can finalize updates: {}", allowed);

        let res = future::ok(allowed);
        Box::pin(res)
    }

    pub(crate) fn report_steady(&self) -> Pin<Box<dyn Future<Output = Result<bool, Error>>>> {
        trace!("periodic strategy, report steady: {}", true);

        let res = future::ok(true);
        Box::pin(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{fragments, inputs};
    use tokio::runtime as rt;

    #[test]
    fn test_default() {
        let default = StrategyPeriodic::default();
        assert_eq!(default.schedule.total_length_minutes(), 0);
    }

    #[test]
    fn test_empty_can_finalize() {
        let default = StrategyPeriodic::default();
        let runtime = rt::Runtime::new().unwrap();
        let steady = runtime.block_on(default.can_finalize()).unwrap();
        assert!(!steady);
    }

    #[test]
    fn test_report_steady() {
        let default = StrategyPeriodic::default();
        let runtime = rt::Runtime::new().unwrap();
        let steady = runtime.block_on(default.report_steady()).unwrap();
        assert!(steady);
    }

    #[test]
    fn test_periodic_config() {
        let cfg = parse_config_input("tests/fixtures/20-periodic-sample.toml");
        let strategy = StrategyPeriodic::new(cfg.updates).unwrap();
        assert_eq!(strategy.schedule.total_length_minutes(), 3145);
    }

    #[test]
    fn test_non_utc_time() {
        use chrono::{Datelike, Timelike};

        // Build a strategy that uses a non UTC time.
        // Time zone is `America/Toronto` in `30-periodic-sample-non-utc.toml`.
        let non_utc_time_cfg = parse_config_input("tests/fixtures/30-periodic-sample-non-utc.toml");
        // Create current datetime with non UTC time.
        let naive_utc_dt = Utc::now().naive_utc();
        let tz = Tz::named(&non_utc_time_cfg.updates.periodic.time_zone).unwrap();
        let dt = (&tz).from_utc_datetime(&naive_utc_dt);
        let weekday = dt.weekday();
        let time = format!("{}:{}", dt.hour(), dt.minute());
        // Modify time windows to only allow naive time in non-UTC time zone's current and following minute.
        let mut non_utc_time_update_input: inputs::UpdateInput = non_utc_time_cfg.updates;
        non_utc_time_update_input.periodic.intervals = vec![inputs::PeriodicIntervalInput {
            start_day: weekday.to_string(),
            start_time: time,
            length_minutes: 2,
        }];

        // Build a strategy that uses UTC.
        // Time zone is not specified in `20-periodic-sample.toml` and so defaults to UTC.
        let utc_cfg = parse_config_input("tests/fixtures/20-periodic-sample.toml");
        // Modify time windows to only allow naive time in non-UTC time zone's current and following minute.
        let mut utc_update_input: inputs::UpdateInput = utc_cfg.updates;
        utc_update_input.periodic.intervals = non_utc_time_update_input.periodic.intervals.clone();

        let non_utc_strategy = StrategyPeriodic::new(non_utc_time_update_input).unwrap();
        let runtime = rt::Runtime::new().unwrap();
        let steady = runtime.block_on(non_utc_strategy.can_finalize()).unwrap();
        assert_eq!(
            non_utc_strategy.time_zone,
            Tz::named("America/Toronto").unwrap()
        );
        // Check that strategy allows reboot now.
        assert!(steady);

        let utc_strategy = StrategyPeriodic::new(utc_update_input).unwrap();
        let runtime = rt::Runtime::new().unwrap();
        let steady = runtime.block_on(utc_strategy.can_finalize()).unwrap();
        assert_eq!(utc_strategy.time_zone, Tz::named("UTC").unwrap());
        // Check that reboot is NOT allowed for UTC strategy.
        assert!(!steady);
    }

    #[test]
    fn test_localtime() {
        use std::matches;
        use std::path::Path;
        let local_time_path = Path::new("/etc/localtime");
        let expected_tz;
        // If symlink `/etc/localtime` doesn't exist, we expect to default to UTC.
        if read_link(local_time_path).is_err() {
            expected_tz = Some(Tz::named("UTC").unwrap());
        } else if let Ok(tz_path) = local_time_path.canonicalize() {
            if tz_path.starts_with("/run/host") {
                // Likely running in a toolbx container on a dev machine, where
                // `/etc/localtime` symlinks into the host mounts. There's
                // no point trying to work around this; the periodic strategy
                // itself will also try to resolve it and fail and it feels
                // awkward to add a toolbx specific hack there too.
                return;
            }
            let tz_str = tz_path
                .strip_prefix(Path::new("/usr/share/zoneinfo"))
                .unwrap()
                .to_str()
                .unwrap();
            expected_tz = Some(Tz::named(tz_str).unwrap());
        } else {
            // `/etc/localtime` exists but points to an invalid time zone.
            expected_tz = None;
        }
        let config = parse_config_input("tests/fixtures/31-periodic-sample-non-utc.toml");
        let strategy = StrategyPeriodic::new(config.updates);
        match expected_tz {
            Some(tz) => assert_eq!(strategy.unwrap().time_zone, tz),
            // If we couldn't canonicalize `/etc/localtime` i.e. it points to an invalid
            // location, make sure that we fail to create a new `StrategyPeriodic` struct.
            None => assert!(matches!(strategy, Err { .. })),
        }
    }

    fn parse_config_input(config_path: &str) -> inputs::ConfigInput {
        let content = std::fs::read_to_string(config_path).unwrap();
        let frag: fragments::ConfigFragment = toml::from_str(&content).unwrap();
        inputs::ConfigInput::merge_fragments(vec![frag])
    }
}
