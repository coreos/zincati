//! Update agent.

mod actor;

use crate::cincinnati::{Cincinnati, Node};
use crate::config::Settings;
use crate::identity::Identity;
use crate::strategy::UpdateStrategy;
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
    UpdateAvailable(Node),
    // TODO(lucab): add all the "update in progress" states.
    /// Final state upon actor end.
    _EndState,
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

    fn update_available(&mut self, update: Option<Node>) {
        // Allowed starting states.
        assert!(*self == UpdateAgentState::Steady);

        if let Some(release) = update {
            *self = UpdateAgentState::UpdateAvailable(release)
        };
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
    /// Update strategy.
    strategy: UpdateStrategy,
    /// Current status for agent state machine.
    state: UpdateAgentState,
    /// Timestamp of last state transition.
    state_changed: DateTime<Utc>,
}

impl UpdateAgent {
    /// Build an update agent with the given config.
    pub(crate) fn with_config(cfg: Settings) -> failure::Fallible<Self> {
        let agent = UpdateAgent {
            cincinnati: cfg.cincinnati,
            identity: cfg.identity,
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
    use crate::cincinnati::Node;
    use std::collections::HashMap;

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

        let update = Node {
            version: "v1".to_string(),
            payload: "ostree-checksum".to_string(),
            metadata: HashMap::new(),
        };
        machine.update_available(Some(update));

        // TODO(lucab): complete the full path till reaching EndState.
        // assert_eq!(machine, UpdateAgentState::_EndState);
    }
}
