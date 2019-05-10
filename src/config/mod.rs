//! Configuration parsing and validation.
//!
//! This module contains the following logical entities:
//!  * Fragments: TOML configuration entries.
//!  * Inputs: configuration fragments merged, but not yet validated.
//!  * Settings: validated settings for the agent.

use crate::identity::Identity;
use failure::{Fallible, ResultExt};
use log::debug;
use serde::Serialize;

/// Runtime configuration for the agent.
///
/// It holds validated agent configuration.
#[derive(Debug, Serialize)]
pub(crate) struct Settings {
    pub(crate) identity: Identity,
}

impl Settings {
    /// Assemble runtime settings.
    pub(crate) fn assemble() -> Fallible<Self> {
        /*
        let dirs = vec!["/usr/lib", "/run", "/etc"];
        let cfg = ConfigInput::read_configs(&dirs, crate_name!())?;
        */
        Self::validate(())
    }

    /// Validate config and return a valid agent settings.
    fn validate(_cfg: ()) -> Fallible<Self> {
        /*
        let identity = Identity::with_config(cfg.identity)?;
         */
        let identity =
            Identity::try_default().context("failed to validate agent identity configuration")?;

        let settings = Self { identity };
        debug!("runtime settings: {:?}", settings);

        Ok(settings)
    }
}
