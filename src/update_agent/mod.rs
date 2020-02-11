//! Update agent.

mod actor;

use crate::cincinnati::Cincinnati;
use crate::config::Settings;
use crate::identity::Identity;
use crate::rpm_ostree::{Release, RpmOstreeClient};
use crate::strategy::UpdateStrategy;
use actix::Addr;
use chrono::prelude::*;
use prometheus::IntGauge;
use std::time::Duration;

/// Default tick/refresh period for the state machine (in seconds).
const DEFAULT_REFRESH_PERIOD_SECS: u64 = 300; // 5 minutes.
/// Default refresh interval for steady state (in seconds).
pub(crate) const DEFAULT_STEADY_INTERVAL_SECS: u64 = 300; // 5 minutes.

lazy_static::lazy_static! {
    static ref LATEST_STATE_CHANGE: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_latest_state_change_timestamp",
        "UTC timestamp of update-agent last state change."
    )).unwrap();
    static ref UPDATES_ENABLED: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_updates_enabled",
        "Whether auto-updates logic is enabled."
    )).unwrap();
}

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
        let start_state = UpdateAgentState::StartState;
        LATEST_STATE_CHANGE.set(chrono::Utc::now().timestamp());
        start_state
    }
}

impl UpdateAgentState {
    /// Return the discriminant for current state
    fn discriminant(&self) -> u8 {
        // TODO(lucab): update when arbitrary-discriminant is stabilized:
        // https://github.com/rust-lang/rust/issues/60553

        match self {
            UpdateAgentState::StartState => 0,
            UpdateAgentState::Initialized => 1,
            UpdateAgentState::Steady => 2,
            UpdateAgentState::UpdateAvailable(_) => 3,
            UpdateAgentState::UpdateStaged(_) => 4,
            UpdateAgentState::UpdateFinalized(_) => 5,
            UpdateAgentState::EndState => 6,
        }
    }

    /// Progress the machine to a new state.
    fn transition_to(&mut self, state: Self) {
        LATEST_STATE_CHANGE.set(chrono::Utc::now().timestamp());

        *self = state;
    }

    /// Transition to the Initialized state.
    fn initialized(&mut self) {
        // Allowed starting states.
        assert!(self.discriminant() == 0);

        self.transition_to(UpdateAgentState::Initialized);
    }

    /// Transition to the Steady state.
    fn steady(&mut self) {
        // Allowed starting states.
        assert!(*self == UpdateAgentState::Initialized);

        self.transition_to(UpdateAgentState::Steady);
    }

    /// Transition to the UpdateAvailable state.
    fn update_available(&mut self, update: Option<Release>) {
        // Allowed starting states.
        assert!(*self == UpdateAgentState::Steady);

        if let Some(release) = update {
            self.transition_to(UpdateAgentState::UpdateAvailable(release));
        }
    }

    /// Transition to the UpdateStaged state.
    fn update_staged(&mut self, update: Release) {
        self.transition_to(UpdateAgentState::UpdateStaged(update));
    }

    /// Transition to the UpdateFinalized state.
    fn update_finalized(&mut self, update: Release) {
        self.transition_to(UpdateAgentState::UpdateFinalized(update));
    }

    /// Transition to the End state.
    fn end(&mut self) {
        self.transition_to(UpdateAgentState::EndState);
    }
}

/// Update agent.
#[derive(Debug)]
pub(crate) struct UpdateAgent {
    /// Whether to allow automatic downgrades.
    allow_downgrade: bool,
    /// Cincinnati service.
    cincinnati: Cincinnati,
    /// Whether to enable auto-updates logic.
    enabled: bool,
    /// Agent identity.
    identity: Identity,
    /// Refresh interval in steady state.
    steady_interval: Duration,
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
        let steady_secs = cfg.steady_interval_secs.get();
        let agent = UpdateAgent {
            allow_downgrade: cfg.allow_downgrade,
            cincinnati: cfg.cincinnati,
            enabled: cfg.enabled,
            identity: cfg.identity,
            rpm_ostree_actor: rpm_ostree_addr,
            steady_interval: Duration::from_secs(steady_secs),
            state: UpdateAgentState::default(),
            strategy: cfg.strategy,
            state_changed: chrono::Utc::now(),
        };

        // TODO(lucab): consider adding more metrics here
        //  (e.g. steady interval, downgrade allowed, etc.)
        UPDATES_ENABLED.set(i64::from(cfg.enabled));

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

        machine.steady();
        assert_eq!(machine, UpdateAgentState::Steady);

        machine.update_available(None);
        assert_eq!(machine, UpdateAgentState::Steady);

        let update = Release {
            version: "v1".to_string(),
            checksum: "ostree-checksum".to_string(),
            age_index: None,
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
