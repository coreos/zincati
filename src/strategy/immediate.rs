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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use tokio::runtime::current_thread as rt;

    #[test]
    fn default() {
        let default = StrategyImmediate::default();
        assert_eq!(default.check, true);
        assert_eq!(default.finalize, true);
    }

    #[test]
    fn report_steady() {
        let default = StrategyImmediate::default();
        let steady = rt::block_on_all(default.report_steady()).unwrap();
        assert_eq!(steady, true);
    }

    proptest! {
        #[test]
        fn proptest_config(check in any::<bool>(), finalize in any::<bool>()){
            let input = StratImmediateInput {
                fetch_updates: Some(check),
                finalize_updates: Some(finalize),
            };

            let strat = StrategyImmediate::with_config(input).unwrap();
            assert_eq!(strat.check, check);
            assert_eq!(strat.finalize, finalize);
        }

        #[test]
        fn proptest_can_check(check in any::<bool>(), finalize in any::<bool>()){
            let input = StratImmediateInput {
                fetch_updates: Some(check),
                finalize_updates: Some(finalize),
            };

            let strat = StrategyImmediate::with_config(input).unwrap();
            let can_check = rt::block_on_all(strat.can_check_and_fetch()).unwrap();
            assert_eq!(can_check, check);
        }

        #[test]
        fn proptest_can_finalize(check in any::<bool>(), finalize in any::<bool>()){
            let input = StratImmediateInput {
                fetch_updates: Some(check),
                finalize_updates: Some(finalize),
            };

            let strat = StrategyImmediate::with_config(input).unwrap();
            let can_finalize = rt::block_on_all(strat.can_finalize()).unwrap();
            assert_eq!(can_finalize, finalize);
        }
    }
}
