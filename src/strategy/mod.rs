//! Update and reboot strategies.

use crate::config::inputs;
use crate::identity::Identity;
use failure::{bail, Fallible};
use futures::prelude::*;
use log::error;
use prometheus::{IntGauge, IntGaugeVec};
use serde::Serialize;

mod fleet_lock;
pub(crate) use fleet_lock::StrategyFleetLock;

mod immediate;
pub(crate) use immediate::StrategyImmediate;

mod periodic;
pub(crate) use periodic::StrategyPeriodic;

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
}

#[derive(Clone, Debug, Serialize)]
pub(crate) enum UpdateStrategy {
    FleetLock(StrategyFleetLock),
    Immediate(StrategyImmediate),
    Periodic(StrategyPeriodic),
}

impl UpdateStrategy {
    /// Try to parse config inputs into a valid strategy.
    pub(crate) fn with_config(cfg: inputs::UpdateInput, identity: &Identity) -> Fallible<Self> {
        let strategy_name = cfg.strategy.clone();
        let strategy = match strategy_name.as_ref() {
            StrategyFleetLock::LABEL => UpdateStrategy::new_fleet_lock(cfg, identity)?,
            StrategyImmediate::LABEL => UpdateStrategy::new_immediate()?,
            StrategyPeriodic::LABEL => UpdateStrategy::new_periodic(cfg)?,
            "" => UpdateStrategy::default(),
            x => bail!("unsupported strategy '{}'", x),
        };

        Ok(strategy)
    }

    /// Record strategy details to metrics and logs.
    pub(crate) fn record_details(&self) {
        // Export info-metrics with details about current strategy.
        STRATEGY_MODE
            .with_label_values(&[self.configuration_label()])
            .set(1);

        if let UpdateStrategy::Periodic(p) = self {
            let sched_length = p.schedule_length_minutes();
            PERIODIC_LENGTH.set(sched_length as i64);
        };

        log::info!("update strategy: {}", self.human_description());
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
        }
    }

    /// Return the human description for this strategy.
    pub(crate) fn human_description(&self) -> String {
        match self {
            UpdateStrategy::FleetLock(_) => self.configuration_label().to_string(),
            UpdateStrategy::Immediate(_) => self.configuration_label().to_string(),
            UpdateStrategy::Periodic(p) => format!(
                "{}, total schedule length {} minutes (next window {})",
                self.configuration_label(),
                p.schedule_length_minutes(),
                p.human_remaining()
            ),
        }
    }

    /// Check if finalization is allowed at this time.
    pub(crate) fn can_finalize(&self) -> impl Future<Output = bool> {
        let lock = match self {
            UpdateStrategy::FleetLock(s) => s.can_finalize(),
            UpdateStrategy::Immediate(s) => s.can_finalize(),
            UpdateStrategy::Periodic(s) => s.can_finalize(),
        };

        async {
            lock.await.unwrap_or_else(|e| {
                error!("{}", e);
                false
            })
        }
    }

    /// Try to report and enter steady state.
    pub(crate) fn report_steady(&self) -> impl Future<Output = bool> {
        let unlock = match self {
            UpdateStrategy::FleetLock(s) => s.report_steady(),
            UpdateStrategy::Immediate(s) => s.report_steady(),
            UpdateStrategy::Periodic(s) => s.report_steady(),
        };

        async {
            unlock.await.unwrap_or_else(|e| {
                error!("{}", e);
                false
            })
        }
    }

    /// Build a new "immediate" strategy.
    fn new_immediate() -> Fallible<Self> {
        let immediate = StrategyImmediate::default();
        Ok(UpdateStrategy::Immediate(immediate))
    }

    /// Build a new "fleet_lock" strategy.
    fn new_fleet_lock(cfg: inputs::UpdateInput, identity: &Identity) -> Fallible<Self> {
        let fleet_lock = StrategyFleetLock::new(cfg, identity)?;
        Ok(UpdateStrategy::FleetLock(fleet_lock))
    }

    /// Build a new "periodic" strategy.
    fn new_periodic(cfg: inputs::UpdateInput) -> Fallible<Self> {
        let periodic = StrategyPeriodic::new(cfg)?;
        Ok(UpdateStrategy::Periodic(periodic))
    }
}

impl Default for UpdateStrategy {
    fn default() -> Self {
        let immediate = StrategyImmediate::default();
        UpdateStrategy::Immediate(immediate)
    }
}
