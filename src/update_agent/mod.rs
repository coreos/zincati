//! Update agent.

mod actor;

use crate::cincinnati::Cincinnati;
use crate::config::Settings;
use crate::identity::Identity;
use crate::rpm_ostree::{Release, RpmOstreeClient};
use crate::strategy::UpdateStrategy;
use actix::Addr;
use chrono::prelude::*;
use std::time::Duration;

/// Default tick/refresh period for the state machine (in seconds).
const DEFAULT_REFRESH_PERIOD_SECS: u64 = 5 * 60;

/// State machine for the agent.
#[derive(Clone, Debug, PartialEq, Eq)]
enum UpdateAgentState {
    /// Initial state upon actor start.
    StartState,
    /// Agent initialized.
    Initialized,
    /// Agent ready to check for updates.
    Steady,
    /// Update available from Cincinnati.
    UpdateAvailable(Release),
    /// Update staged by rpm-ostree.
    UpdateStaged(Release),
    /// Update finalized by rpm-ostree.
    UpdateFinalized(Release),
    /// Final state upon actor end.
    EndState,
}

impl Default for UpdateAgentState {
    fn default() -> Self {
        UpdateAgentState::StartState
    }
}

impl UpdateAgentState {
    /// Transition to the Initialized state.
    fn initialized(&mut self) {
        // Allowed starting states.
        assert!(*self == UpdateAgentState::StartState);

        *self = UpdateAgentState::Initialized;
    }

    /// Transition to the Steady state.
    fn steady(&mut self, is_steady: bool) {
        // Allowed starting states.
        assert!(*self == UpdateAgentState::Initialized);

        if is_steady {
            *self = UpdateAgentState::Steady;
        }
    }

    /// Transition to the UpdateAvailable state.
    fn update_available(&mut self, update: Option<Release>) {
        // Allowed starting states.
        assert!(*self == UpdateAgentState::Steady);

        if let Some(release) = update {
            *self = UpdateAgentState::UpdateAvailable(release)
        };
    }

    /// Transition to the UpdateStaged state.
    fn update_staged(&mut self, update: Release) {
        *self = UpdateAgentState::UpdateStaged(update);
    }

    /// Transition to the UpdateFinalized state.
    fn update_finalized(&mut self, update: Release) {
        *self = UpdateAgentState::UpdateFinalized(update);
    }

    /// Transition to the End state.
    fn end(&mut self) {
        *self = UpdateAgentState::EndState;
    }
}

/// Update agent.
#[derive(Debug)]
pub(crate) struct UpdateAgent {
    /// Cincinnati service.
    cincinnati: Cincinnati,
    /// Agent identity.
    identity: Identity,
    /// State machine tick/refresh period.
    refresh_period: Duration,
    /// rpm-ostree client actor.
    rpm_ostree_actor: Addr<RpmOstreeClient>,
    /// Update strategy.
    strategy: UpdateStrategy,
    /// Current status for agent state machine.
    state: UpdateAgentState,
    /// Timestamp of last state transition.
    state_changed: DateTime<Utc>,
}

impl UpdateAgent {
    /// Build an update agent with the given config.
    pub(crate) fn with_config(
        cfg: Settings,
        rpm_ostree_addr: Addr<RpmOstreeClient>,
    ) -> failure::Fallible<Self> {
        let agent = UpdateAgent {
            cincinnati: cfg.cincinnati,
            identity: cfg.identity,
            rpm_ostree_actor: rpm_ostree_addr,
            // TODO(lucab): consider tweaking this
            //   * maybe configurable, in minutes?
            //   * maybe more granular, per-state?
            refresh_period: Duration::from_secs(DEFAULT_REFRESH_PERIOD_SECS),
            state: UpdateAgentState::default(),
            strategy: cfg.strategy,
            state_changed: chrono::Utc::now(),
        };
        Ok(agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpm_ostree::Release;

    #[test]
    fn default_state() {
        assert_eq!(UpdateAgentState::default(), UpdateAgentState::StartState);
    }

    #[test]
    fn state_machine_happy_path() {
        let mut machine = UpdateAgentState::default();
        assert_eq!(machine, UpdateAgentState::StartState);

        machine.initialized();
        assert_eq!(machine, UpdateAgentState::Initialized);

        machine.steady(true);
        assert_eq!(machine, UpdateAgentState::Steady);

        machine.update_available(None);
        assert_eq!(machine, UpdateAgentState::Steady);

        let update = Release {
            version: "v1".to_string(),
            checksum: "ostree-checksum".to_string(),
        };
        machine.update_available(Some(update.clone()));
        assert_eq!(machine, UpdateAgentState::UpdateAvailable(update.clone()));

        machine.update_staged(update.clone());
        assert_eq!(machine, UpdateAgentState::UpdateStaged(update.clone()));

        machine.update_finalized(update.clone());
        assert_eq!(machine, UpdateAgentState::UpdateFinalized(update.clone()));

        machine.end();
        assert_eq!(machine, UpdateAgentState::EndState);
    }
}
