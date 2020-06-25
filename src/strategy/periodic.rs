//! Strategy for periodic (weekly) updates.

use crate::config::inputs;
use crate::weekly::{utils, WeeklyCalendar, WeeklyWindow};
use failure::{Error, Fallible};
use futures::future;
use futures::prelude::*;
use log::trace;
use serde::Serialize;
use std::pin::Pin;
use std::time::Duration;

/// Strategy for periodic (weekly) updates.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct StrategyPeriodic {
    /// Whitelisted time windows during which updates are allowed.
    schedule: WeeklyCalendar,
}

impl StrategyPeriodic {
    /// Strategy label/name.
    pub const LABEL: &'static str = "periodic";

    /// Build a new periodic strategy.
    pub fn new(cfg: inputs::UpdateInput) -> Fallible<Self> {
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
            0 => failure::bail!(
                "invalid or missing periodic updates configuration: weekly calendar length is zero"
            ),
            n => log::trace!("periodic updates, weekly calendar length: {} minutes", n),
        };

        let strategy = Self { schedule: calendar };
        Ok(strategy)
    }

    /// Return the measured length of the schedule, in minutes.
    pub(crate) fn schedule_length_minutes(&self) -> u64 {
        self.schedule.length_minutes()
    }

    /// Check if finalization is allowed.
    pub(crate) fn can_finalize(&self) -> Pin<Box<dyn Future<Output = Result<bool, Error>>>> {
        let datetime_now = chrono::Utc::now();
        let allowed = self.schedule.contains_datetime(&datetime_now);

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
    use std::io::Read;
    use tokio::runtime as rt;

    #[test]
    fn test_default() {
        let default = StrategyPeriodic::default();
        assert_eq!(default.schedule.total_length_minutes(), 0);
    }

    #[test]
    fn test_empty_can_finalize() {
        let default = StrategyPeriodic::default();
        let mut runtime = rt::Runtime::new().unwrap();
        let steady = runtime.block_on(default.can_finalize()).unwrap();
        assert_eq!(steady, false);
    }

    #[test]
    fn test_report_steady() {
        let default = StrategyPeriodic::default();
        let mut runtime = rt::Runtime::new().unwrap();
        let steady = runtime.block_on(default.report_steady()).unwrap();
        assert_eq!(steady, true);
    }

    #[test]
    fn test_periodic_config() {
        let fp = std::fs::File::open("tests/fixtures/20-periodic-sample.toml").unwrap();
        let mut bufrd = std::io::BufReader::new(fp);
        let mut content = vec![];
        bufrd.read_to_end(&mut content).unwrap();
        let frag: fragments::ConfigFragment = toml::from_slice(&content).unwrap();
        let cfg = inputs::ConfigInput::merge_fragments(vec![frag]);

        let strategy = StrategyPeriodic::new(cfg.updates).unwrap();
        assert_eq!(strategy.schedule.total_length_minutes(), 3145);
    }
}
