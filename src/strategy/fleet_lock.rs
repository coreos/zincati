//! Strategy for fleet-wide coordinated updates (FleetLock protocol).

use crate::config::inputs;
use crate::fleet_lock::{Client, ClientBuilder};
use crate::identity::Identity;
use failure::{Error, Fallible};
use futures::future;
use futures::prelude::*;
use log::trace;
use serde::Serialize;

/// Strategy for remote coordination.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StrategyFleetLock {
    /// Asynchronous client.
    pub(crate) client: Client,
}

impl StrategyFleetLock {
    /// Build a new FleetLock strategy.
    pub fn new(cfg: inputs::UpdateInput, identity: &Identity) -> Fallible<Self> {
        // Substitute templated key with agent runtime values.
        let base_url = if envsubst::is_templated(&cfg.fleet_lock.base_url) {
            let context = identity.url_variables();
            envsubst::validate_vars(&context)?;
            envsubst::substitute(cfg.fleet_lock.base_url, &context)?
        } else {
            cfg.fleet_lock.base_url
        };

        if base_url.is_empty() {
            failure::bail!("empty fleet_lock base URL");
        }
        log::info!("remote fleet_lock reboot manager: {}", &base_url);

        let builder = ClientBuilder::new(base_url, identity);
        let client = builder.build()?;
        let strategy = Self { client };
        Ok(strategy)
    }

    /// Check if finalization is allowed.
    pub(crate) fn can_finalize(&self) -> Box<dyn Future<Item = bool, Error = Error>> {
        trace!("fleet_lock strategy, checking whether update can be finalized");

        Box::new(self.client.pre_reboot())
    }

    /// Try to report steady state.
    pub(crate) fn report_steady(&self) -> Box<dyn Future<Item = bool, Error = Error>> {
        trace!("fleet_lock strategy, attempting to report steady");

        Box::new(self.client.steady_state())
    }

    /// Check if fetching updates is allowed
    pub(crate) fn can_check_and_fetch(&self) -> Box<dyn Future<Item = bool, Error = Error>> {
        trace!("fleet_lock strategy, can check updates: {}", true);

        // TODO(lucab): https://github.com/coreos/zincati/issues/35
        let res = future::ok(true);
        Box::new(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::inputs::{FleetLockInput, UpdateInput};
    use crate::identity::Identity;

    #[test]
    fn test_url_simple() {
        let id = Identity::mock_default();
        let input = UpdateInput {
            enabled: true,
            strategy: "fleet_lock".to_string(),
            fleet_lock: FleetLockInput {
                base_url: "https://example.com".to_string(),
            },
        };

        let res = StrategyFleetLock::new(input, &id);
        assert!(res.is_ok());
    }

    #[test]
    fn test_empty_url() {
        let id = Identity::mock_default();
        let input = UpdateInput {
            enabled: true,
            strategy: "fleet_lock".to_string(),
            fleet_lock: FleetLockInput {
                base_url: String::new(),
            },
        };

        let res = StrategyFleetLock::new(input, &id);
        assert!(res.is_err());
    }
}
