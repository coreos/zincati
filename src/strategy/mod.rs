//! Update and reboot strategies.

use crate::config::inputs;
use crate::identity::Identity;
use anyhow::Result;
use fn_error_context::context;
use futures::prelude::*;
use log::error;
use prometheus::{IntCounterVec, IntGauge, IntGaugeVec};
use serde::Serialize;

mod fleet_lock;
pub(crate) use fleet_lock::StrategyFleetLock;

mod immediate;
pub(crate) use immediate::StrategyImmediate;

mod periodic;
pub(crate) use periodic::StrategyPeriodic;

mod marker_file;
pub(crate) use marker_file::StrategyMarkerFile;

/// Label for allow responses from querying strategy's `can_finalize` function.
pub static CAN_FINALIZE_ALLOW_LABEL: &str = "allow";

/// Label for deny responses from querying strategy's `can_finalize` function.
pub static CAN_FINALIZE_DENY_LABEL: &str = "deny";

/// Label for error responses from querying strategy's `can_finalize` function.
pub static CAN_FINALIZE_ERROR_LABEL: &str = "error";

lazy_static::lazy_static! {
    static ref STRATEGY_MODE: IntGaugeVec = register_int_gauge_vec!(
        "zincati_updates_strategy_mode",
        "Update strategy mode in use",
        &["strategy"]
    ).unwrap();

    static ref PERIODIC_LENGTH: IntGauge = register_int_gauge!(
        "zincati_updates_strategy_periodic_schedule_length_minutes",
        "Total length of the periodic strategy schedule in use"
    ).unwrap();

    static ref FINALIZATION_STRATEGY_RESPONSES: IntCounterVec = register_int_counter_vec!(
        "zincati_updates_strategy_can_finalize_responses",
        "Total number of responses from querying update strategy for finalization consent.",
        &["response"]
    ).unwrap();
}

#[derive(Clone, Debug, Serialize)]
pub(crate) enum UpdateStrategy {
    FleetLock(StrategyFleetLock),
    Immediate(StrategyImmediate),
    Periodic(StrategyPeriodic),
    MarkerFile(StrategyMarkerFile),
}

impl UpdateStrategy {
    /// Try to parse config inputs into a valid strategy.
    #[context("failed to validate update strategy configuration")]
    pub(crate) fn with_config(cfg: inputs::UpdateInput, identity: &Identity) -> Result<Self> {
        let strategy_name = cfg.strategy.clone();
        let strategy = match strategy_name.as_ref() {
            StrategyFleetLock::LABEL => UpdateStrategy::new_fleet_lock(cfg, identity)?,
            StrategyImmediate::LABEL => UpdateStrategy::new_immediate(),
            StrategyPeriodic::LABEL => UpdateStrategy::new_periodic(cfg)?,
            StrategyMarkerFile::LABEL => UpdateStrategy::new_marker_file(),
            "" => UpdateStrategy::default(),
            x => anyhow::bail!("unsupported strategy '{}'", x),
        };

        Ok(strategy)
    }

    /// Record strategy details to metrics and logs.
    pub(crate) fn record_details(&self) {
        self.refresh_metrics();
        log::info!("update strategy: {}", self.human_description());
    }

    /// Refresh strategy-related metrics values.
    pub(crate) fn refresh_metrics(&self) {
        // Export info-metrics with details about current strategy.
        STRATEGY_MODE
            .with_label_values(&[self.configuration_label()])
            .set(1);

        if let UpdateStrategy::Periodic(p) = self {
            let sched_length = p.schedule_length_minutes();
            PERIODIC_LENGTH.set(sched_length as i64);
        };
    }

    /// Return the configuration label/name for this update strategy.
    ///
    /// This can be used to match back an instantiated strategy to the mode label
    /// from configuration.
    fn configuration_label(&self) -> &'static str {
        match self {
            UpdateStrategy::FleetLock(_) => StrategyFleetLock::LABEL,
            UpdateStrategy::Immediate(_) => StrategyImmediate::LABEL,
            UpdateStrategy::Periodic(_) => StrategyPeriodic::LABEL,
            UpdateStrategy::MarkerFile(_) => StrategyMarkerFile::LABEL,
        }
    }

    /// Return the human description for this strategy.
    pub(crate) fn human_description(&self) -> String {
        match self {
            UpdateStrategy::FleetLock(_) => self.configuration_label().to_string(),
            UpdateStrategy::Immediate(_) => self.configuration_label().to_string(),
            UpdateStrategy::Periodic(p) => {
                format!("{}, {}", self.configuration_label(), p.calendar_summary(),)
            }
            UpdateStrategy::MarkerFile(_) => self.configuration_label().to_string(),
        }
    }

    /// Check if finalization is allowed at this time.
    pub(crate) fn can_finalize(&self) -> impl Future<Output = bool> {
        let lock = match self {
            UpdateStrategy::FleetLock(s) => s.can_finalize(),
            UpdateStrategy::Immediate(s) => s.can_finalize(),
            UpdateStrategy::Periodic(s) => s.can_finalize(),
            UpdateStrategy::MarkerFile(s) => s.can_finalize(),
        };

        async {
            match lock.await {
                Ok(can_finalize) => {
                    if can_finalize {
                        FINALIZATION_STRATEGY_RESPONSES
                            .with_label_values(&[CAN_FINALIZE_ALLOW_LABEL])
                            .inc();
                    } else {
                        FINALIZATION_STRATEGY_RESPONSES
                            .with_label_values(&[CAN_FINALIZE_DENY_LABEL])
                            .inc();
                    }
                    can_finalize
                }
                Err(e) => {
                    FINALIZATION_STRATEGY_RESPONSES
                        .with_label_values(&[CAN_FINALIZE_ERROR_LABEL])
                        .inc();
                    error!("{}", e);
                    false
                }
            }
        }
    }

    /// Try to report and enter steady state.
    pub(crate) fn report_steady(&self) -> impl Future<Output = bool> {
        let unlock = match self {
            UpdateStrategy::FleetLock(s) => s.report_steady(),
            UpdateStrategy::Immediate(s) => s.report_steady(),
            UpdateStrategy::Periodic(s) => s.report_steady(),
            UpdateStrategy::MarkerFile(s) => s.report_steady(),
        };

        async {
            unlock.await.unwrap_or_else(|e| {
                error!("{}", e);
                false
            })
        }
    }

    /// Build a new "immediate" strategy.
    fn new_immediate() -> Self {
        let immediate = StrategyImmediate::default();
        UpdateStrategy::Immediate(immediate)
    }

    /// Build a new "fleet_lock" strategy.
    fn new_fleet_lock(cfg: inputs::UpdateInput, identity: &Identity) -> Result<Self> {
        let fleet_lock = StrategyFleetLock::new(cfg, identity)?;
        Ok(UpdateStrategy::FleetLock(fleet_lock))
    }

    /// Build a new "periodic" strategy.
    fn new_periodic(cfg: inputs::UpdateInput) -> Result<Self> {
        let periodic = StrategyPeriodic::new(cfg)?;
        Ok(UpdateStrategy::Periodic(periodic))
    }

    /// Build a new "filesystem" strategy.
    fn new_marker_file() -> Self {
        let marker_file = StrategyMarkerFile::default();
        UpdateStrategy::MarkerFile(marker_file)
    }
}

impl Default for UpdateStrategy {
    fn default() -> Self {
        let immediate = StrategyImmediate::default();
        UpdateStrategy::Immediate(immediate)
    }
}
