//! Strategy for immediate updates.

use anyhow::Error;
use futures::future;
use futures::prelude::*;
use log::trace;
use serde::Serialize;
use std::pin::Pin;

/// Strategy for immediate updates.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct StrategyImmediate {}

impl StrategyImmediate {
    /// Strategy label/name.
    pub const LABEL: &'static str = "immediate";

    /// Check if finalization is allowed.
    pub(crate) fn can_finalize(&self) -> Pin<Box<dyn Future<Output = Result<bool, Error>>>> {
        trace!("immediate strategy, can finalize updates: {}", true);

        let res = future::ok(true);
        Box::pin(res)
    }

    pub(crate) fn report_steady(&self) -> Pin<Box<dyn Future<Output = Result<bool, Error>>>> {
        trace!("immediate strategy, report steady: {}", true);

        let immediate = future::ok(true);
        Box::pin(immediate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime as rt;

    #[test]
    fn report_steady() {
        let default = StrategyImmediate::default();
        let runtime = rt::Runtime::new().unwrap();
        let steady = runtime.block_on(default.report_steady()).unwrap();
        assert!(steady);
    }

    #[test]
    fn can_finalize() {
        let default = StrategyImmediate::default();
        let runtime = rt::Runtime::new().unwrap();
        let can_finalize = runtime.block_on(default.can_finalize()).unwrap();
        assert!(can_finalize);
    }
}
