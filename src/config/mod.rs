//! Configuration parsing and validation.
//!
//! This module contains the following logical entities:
//!  * Fragments: TOML configuration entries.
//!  * Inputs: configuration fragments merged, but not yet validated.
//!  * Settings: validated settings for the agent.

/// TOML structures.
mod fragments;

/// Configuration fragments.
pub(crate) mod inputs;

use crate::identity::Identity;
use crate::strategy::UpdateStrategy;
use failure::{Fallible, ResultExt};
use serde::Serialize;
use structopt::clap::crate_name;

/// Runtime configuration for the agent.
///
/// It holds validated agent configuration.
#[derive(Debug, Serialize)]
pub(crate) struct Settings {
    /// Agent configuration.
    pub(crate) identity: Identity,
    /// Agent update strategy.
    pub(crate) strategy: UpdateStrategy,
}

impl Settings {
    /// Assemble runtime settings.
    pub(crate) fn assemble() -> Fallible<Self> {
        let dirs = vec!["/usr/lib", "/run", "/etc"];
        let cfg = inputs::ConfigInput::read_configs(&dirs, crate_name!())?;
        Self::validate(cfg)
    }

    /// Validate config and return a valid agent settings.
    fn validate(cfg: inputs::ConfigInput) -> Fallible<Self> {
        let identity = Identity::with_config(cfg.identity)
            .context("failed to validate agent identity configuration")?;
        let strategy = UpdateStrategy::with_config(cfg.updates)
            .context("failed to validate agent updates configuration")?;

        Ok(Self { identity, strategy })
    }
}
