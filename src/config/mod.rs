//! Configuration parsing and validation.
//!
//! This module contains the following logical entities:
//!  * Fragments: TOML configuration entries.
//!  * Inputs: configuration fragments merged, but not yet validated.
//!  * Settings: validated settings for the agent.

/// TOML structures.
pub(crate) mod fragments;

/// Configuration fragments.
pub(crate) mod inputs;

use crate::cincinnati::Cincinnati;
use crate::identity::Identity;
use crate::strategy::UpdateStrategy;
use crate::update_agent;
use failure::{Fallible, ResultExt};
use serde::Serialize;
use std::num::NonZeroU64;
use structopt::clap::crate_name;

/// Runtime configuration for the agent.
///
/// It holds validated agent configuration.
#[derive(Debug, Serialize)]
pub(crate) struct Settings {
    /// Whether to enable automatic downgrades.
    pub(crate) allow_downgrade: bool,
    /// Whether to enable auto-updates logic.
    pub(crate) enabled: bool,
    /// Agent timing, steady state refresh period.
    pub(crate) steady_interval_secs: NonZeroU64,
    /// Cincinnati configuration.
    pub(crate) cincinnati: Cincinnati,
    /// Agent configuration.
    pub(crate) identity: Identity,
    /// Agent update strategy.
    pub(crate) strategy: UpdateStrategy,
}

impl Settings {
    /// Assemble runtime settings.
    pub(crate) fn assemble() -> Fallible<Self> {
        let prefixes = vec![
            "/usr/lib/".to_string(),
            "/run/".to_string(),
            "/etc/".to_string(),
        ];
        let common_path = format!("{}/config.d/", crate_name!());
        let extensions = vec!["toml".to_string()];
        let cfg = inputs::ConfigInput::read_configs(prefixes, &common_path, extensions)?;
        Self::validate(cfg)
    }

    /// Refresh settings-related metrics values.
    pub(crate) fn refresh_metrics(&self) {
        // TODO(lucab): consider adding more metrics here (e.g. steady interval).
        update_agent::UPDATES_ENABLED.set(i64::from(self.enabled));
        update_agent::ALLOW_DOWNGRADE.set(i64::from(self.allow_downgrade));

        self.strategy.refresh_metrics();
    }

    /// Validate config and return a valid agent settings.
    fn validate(cfg: inputs::ConfigInput) -> Fallible<Self> {
        let allow_downgrade = cfg.updates.allow_downgrade;
        let enabled = cfg.updates.enabled;
        let steady_interval_secs = cfg.agent.steady_interval_secs;
        let identity = Identity::with_config(cfg.identity)
            .context("failed to validate agent identity configuration")?;
        let strategy = UpdateStrategy::with_config(cfg.updates, &identity)
            .context("failed to validate update-strategy configuration")?;
        let cincinnati = Cincinnati::with_config(cfg.cincinnati, &identity)
            .context("failed to validate cincinnati configuration")?;

        Ok(Self {
            allow_downgrade,
            enabled,
            steady_interval_secs,
            cincinnati,
            identity,
            strategy,
        })
    }
}
