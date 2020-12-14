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

/// Default refresh interval for steady state (in seconds).
pub(crate) const DEFAULT_STEADY_INTERVAL_SECS: u64 = 300; // 5 minutes.

/// Default tick/refresh period for the state machine (in seconds).
const DEFAULT_REFRESH_PERIOD_SECS: u64 = 300; // 5 minutes.

/// Maximum failed deploy attempts in a row in `UpdateAvailable` state
/// before abandoning a target update.
const MAX_DEPLOY_ATTEMPTS: u8 = 12;

lazy_static::lazy_static! {
    pub(crate) static ref ALLOW_DOWNGRADE: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_updates_allow_downgrade",
        "Whether downgrades via auto-updates logic are allowed."
    )).unwrap();
    static ref LATEST_STATE_CHANGE: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_latest_state_change_timestamp",
        "UTC timestamp of update-agent last state change."
    )).unwrap();
    pub(crate) static ref UPDATES_ENABLED: IntGauge = register_int_gauge!(opts!(
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
    /// Node steady, agent allowed to check for updates.
    ReportedSteady,
    /// No further updates available yet.
    NoNewUpdate,
    /// Update available from Cincinnati.
    ///
    /// The integer counter keeps track of how many times in a row this
    /// update was attempted, but deploying failed. At `MAX_DEPLOY_ATTEMPTS`
    /// a state transition is triggered to abandon the target update.
    UpdateAvailable((Release, u8)),
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
    /// Progress the machine to a new state.
    fn transition_to(&mut self, state: Self) {
        LATEST_STATE_CHANGE.set(chrono::Utc::now().timestamp());

        *self = state;
    }

    /// Transition to the Initialized state.
    fn initialized(&mut self) {
        let target = UpdateAgentState::Initialized;
        // Allowed starting states.
        assert!(
            *self == UpdateAgentState::StartState,
            "transition not allowed: {:?} to {:?}",
            self,
            target,
        );

        self.transition_to(target);
    }

    /// Transition to the ReportedSteady state.
    fn reported_steady(&mut self) {
        let target = UpdateAgentState::ReportedSteady;
        // Allowed starting states.
        assert!(
            *self == UpdateAgentState::Initialized,
            "transition not allowed: {:?} to {:?}",
            self,
            target,
        );

        self.transition_to(target);
    }

    /// Transition to the NoNewUpdate state.
    fn no_new_update(&mut self) {
        let target = UpdateAgentState::NoNewUpdate;
        // Allowed starting states.
        assert!(
            *self == UpdateAgentState::ReportedSteady || *self == UpdateAgentState::NoNewUpdate,
            "transition not allowed: {:?} to {:?}",
            self,
            target
        );

        self.transition_to(UpdateAgentState::NoNewUpdate);
    }

    /// Transition to the UpdateAvailable state with a new release.
    fn update_available(&mut self, update: Release) {
        let target = UpdateAgentState::UpdateAvailable((update, 0));
        // Allowed starting states.
        assert!(
            *self == UpdateAgentState::ReportedSteady || *self == UpdateAgentState::NoNewUpdate,
            "transition not allowed: {:?} to {:?}",
            self,
            target
        );

        self.transition_to(target);
    }

    /// Record a failed deploy attempt in UpdateAvailable state.
    ///
    /// This returns whether a persistent deploy failure was detected
    /// and the target update abandoned.
    fn record_failed_deploy(&mut self) -> bool {
        let (release, attempts) = match self.clone() {
            UpdateAgentState::UpdateAvailable((r, a)) => (r, a),
            _ => unreachable!("transition not allowed: record_failed_deploy on {:?}", self,),
        };
        let fail_count = attempts.saturating_add(1);
        let persistent_err = fail_count >= MAX_DEPLOY_ATTEMPTS;

        if persistent_err {
            self.update_abandoned();
        } else {
            self.deploy_failed(release, fail_count);
        }

        persistent_err
    }

    /// Transition to the UpdateAvailable state after a deploy failure.
    fn deploy_failed(&mut self, update: Release, fail_count: u8) {
        let target = UpdateAgentState::UpdateAvailable((update, fail_count));

        self.transition_to(target);
    }

    /// Transition to the NoNewUpdate state after persistent deploy failure.
    fn update_abandoned(&mut self) {
        let target = UpdateAgentState::NoNewUpdate;

        self.transition_to(target);
    }

    /// Transition to the UpdateStaged state.
    fn update_staged(&mut self, update: Release) {
        let target = UpdateAgentState::UpdateStaged(update);

        self.transition_to(target);
    }

    /// Transition to the UpdateFinalized state.
    fn update_finalized(&mut self, update: Release) {
        let target = UpdateAgentState::UpdateFinalized(update);

        self.transition_to(target);
    }

    /// Transition to the End state.
    fn end(&mut self) {
        let target = UpdateAgentState::EndState;

        self.transition_to(target);
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

        machine.reported_steady();
        assert_eq!(machine, UpdateAgentState::ReportedSteady);

        machine.no_new_update();
        assert_eq!(machine, UpdateAgentState::NoNewUpdate);

        machine.no_new_update();
        assert_eq!(machine, UpdateAgentState::NoNewUpdate);

        let update = Release {
            version: "v1".to_string(),
            checksum: "ostree-checksum".to_string(),
            age_index: None,
        };
        machine.update_available(update.clone());
        assert_eq!(
            machine,
            UpdateAgentState::UpdateAvailable((update.clone(), 0))
        );

        let persistent_err = machine.record_failed_deploy();
        assert_eq!(persistent_err, false);
        assert_eq!(
            machine,
            UpdateAgentState::UpdateAvailable((update.clone(), 1))
        );

        machine.update_staged(update.clone());
        assert_eq!(machine, UpdateAgentState::UpdateStaged(update.clone()));

        machine.update_finalized(update.clone());
        assert_eq!(machine, UpdateAgentState::UpdateFinalized(update.clone()));

        machine.end();
        assert_eq!(machine, UpdateAgentState::EndState);
    }

    #[test]
    fn test_fsm_abandon_update() {
        let update = Release {
            version: "v1".to_string(),
            checksum: "ostree-checksum".to_string(),
            age_index: None,
        };
        let mut machine = UpdateAgentState::NoNewUpdate;

        machine.update_available(update.clone());
        assert_eq!(
            machine,
            UpdateAgentState::UpdateAvailable((update.clone(), 0))
        );

        // MAX-1 temporary failures.
        for attempt in 1..MAX_DEPLOY_ATTEMPTS {
            let persistent_err = machine.record_failed_deploy();
            assert_eq!(persistent_err, false);
            assert_eq!(
                machine,
                UpdateAgentState::UpdateAvailable((update.clone(), attempt as u8))
            )
        }

        // Persistent error threshold reached.
        let persistent_err = machine.record_failed_deploy();
        assert_eq!(persistent_err, true);
        assert_eq!(machine, UpdateAgentState::NoNewUpdate);
    }
}
