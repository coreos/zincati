//! Update and reboot strategies.

#![allow(unused)]

use crate::config::inputs;
use crate::identity::Identity;
use failure::{bail, Fallible};
use futures::prelude::*;
use log::error;
use serde::Serialize;

mod immediate;
pub(crate) use immediate::StrategyImmediate;

#[derive(Clone, Debug, Serialize)]
pub(crate) enum UpdateStrategy {
    Immediate(StrategyImmediate),
}

impl UpdateStrategy {
    /// Try to parse config inputs into a valid strategy.
    pub(crate) fn with_config(cfg: inputs::UpdateInput) -> Fallible<Self> {
        let strategy = match cfg.strategy.as_ref() {
            "immediate" => UpdateStrategy::new_immediate()?,
            "" => UpdateStrategy::default(),
            x => bail!("unsupported strategy '{}'", x),
        };
        Ok(strategy)
    }

    /// Check if finalization is allowed at this time.
    pub(crate) fn can_finalize(
        &self,
        _identity: &Identity,
    ) -> Box<Future<Item = bool, Error = ()>> {
        let lock = match self {
            UpdateStrategy::Immediate(i) => i.can_finalize(),
        }
        .map_err(|e| error!("{}", e));
        Box::new(lock)
    }

    /// Try to report and enter steady state.
    pub(crate) fn report_steady(
        &self,
        _identity: &Identity,
    ) -> Box<Future<Item = bool, Error = ()>> {
        let unlock = match self {
            UpdateStrategy::Immediate(i) => i.report_steady(),
        }
        .map_err(|e| error!("{}", e));
        Box::new(unlock)
    }

    /// Check if this agent is allowed to check for updates at this time.
    pub(crate) fn can_check_and_fetch(
        &self,
        _identity: &Identity,
    ) -> Box<Future<Item = bool, Error = ()>> {
        let can_check = match self {
            UpdateStrategy::Immediate(i) => i.can_check_and_fetch(),
        }
        .map_err(|e| error!("{}", e));
        Box::new(can_check)
    }

    fn new_immediate() -> Fallible<Self> {
        let immediate = StrategyImmediate::default();
        Ok(UpdateStrategy::Immediate(immediate))
    }
}

impl Default for UpdateStrategy {
    fn default() -> Self {
        let immediate = StrategyImmediate::default();
        UpdateStrategy::Immediate(immediate)
    }
}
