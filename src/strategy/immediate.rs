//! Strategy for immediate updates.

#![allow(unused)]

use crate::config::inputs::StratImmediateInput;
use failure::{Error, Fallible};
use futures::future;
use futures::prelude::*;
use log::trace;
use serde::Serialize;

/// Strategy for immediate updates.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StrategyImmediate {
    /// Whether to check for and fetch updates.
    check: bool,
    /// Whether to finalize updates.
    finalize: bool,
}

impl StrategyImmediate {
    /// Try to parse strategy configuration.
    pub(crate) fn with_config(cfg: StratImmediateInput) -> Fallible<Self> {
        let mut immediate = Self::default();

        if let Some(check) = cfg.fetch_updates {
            immediate.check = check;
        }
        if let Some(finalize) = cfg.finalize_updates {
            immediate.finalize = finalize;
        }

        Ok(immediate)
    }

    /// Check if finalization is allowed.
    pub(crate) fn can_finalize(&self) -> impl Future<Item = bool, Error = Error> {
        trace!(
            "immediate strategy, can finalize updates: {}",
            self.finalize
        );

        let immediate = future::ok(self.finalize);
        Box::new(immediate)
    }

    pub(crate) fn report_steady(&self) -> Box<Future<Item = bool, Error = Error>> {
        trace!("immediate strategy, report steady: {}", true);

        let immediate = future::ok(true);
        Box::new(immediate)
    }

    pub(crate) fn can_check_and_fetch(&self) -> Box<Future<Item = bool, Error = Error>> {
        trace!("immediate strategy, can check updates: {}", self.check);

        let immediate = future::ok(self.check);
        Box::new(immediate)
    }
}

impl Default for StrategyImmediate {
    fn default() -> Self {
        Self {
            check: true,
            finalize: true,
        }
    }
}
