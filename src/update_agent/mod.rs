//! Update agent.

mod actor;

use crate::cincinnati::Cincinnati;
use crate::config::Settings;
use crate::identity::Identity;
use crate::rpm_ostree::{Release, RpmOstreeClient};
use crate::strategy::UpdateStrategy;
use actix::Addr;
use chrono::prelude::*;
use failure::{bail, Fallible, ResultExt};
use prometheus::IntGauge;
use serde::{Deserialize, Deserializer};
use std::fs;
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

/// JSON output from `loginctl list-sessions --output=json`
#[derive(Debug, Deserialize)]
pub struct SessionsJSON {
    user: String,
    #[serde(deserialize_with = "empty_string_as_none")]
    tty: Option<String>,
}

/// Function to deserialize field to `Option<String>`, where empty strings are
/// deserialized into `None`.
fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
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
        use std::mem::discriminant;
        if discriminant(self) != discriminant(&state) {
            LATEST_STATE_CHANGE.set(chrono::Utc::now().timestamp());
        }

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
    /// This returns a tuple containing a bool representing whether the target
    /// update was abandoned and the total number of failed deployment attempts
    /// (including the newly recorded failed attempt).
    fn record_failed_deploy(&mut self) -> (bool, u8) {
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

        (persistent_err, fail_count)
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

/// Attempt to broadcast msg to all sessions registered in systemd's login manager.
/// Returns a Result with a tuple of total sessions found and sessions broadcasted to,
/// if no error.
fn broadcast(msg: &str) -> Fallible<(usize, usize)> {
    let sessions = get_user_sessions()?;
    let sessions_total = sessions.len();
    let mut sessions_broadcasted: usize = 0;

    let broadcast_msg = format!(
        "\nBroadcast message from Zincati at {}:\n{}\n",
        chrono::Utc::now().format("%a %Y-%m-%d %H:%M:%S %Z"),
        msg
    );

    // Iterate over sessions and attempt to write to each session's tty.
    for session in sessions.into_iter() {
        let user = session.user;
        let tty_dev = match session.tty {
            Some(mut tty) => {
                tty.insert_str(0, "/dev/");
                tty
            }
            None => {
                log::debug!(
                    "found user {} with no tty, skipping broadcast to this user",
                    user
                );
                continue;
            }
        };

        log::trace!(
            "Attempting to broadcast a message to user {} at {}",
            user,
            tty_dev
        );

        {
            if let Err(e) = fs::write(&tty_dev, &broadcast_msg) {
                log::error!("failed to write to {}: {}", &tty_dev, e);
                continue;
            };
        }

        sessions_broadcasted = sessions_broadcasted.saturating_add(1);
    }

    Ok((sessions_total, sessions_broadcasted))
}

/// Get sessions with users logged in using `loginctl`.
/// Returns a Result with vector of `SessionsJSON`, if no error.
fn get_user_sessions() -> Fallible<Vec<SessionsJSON>> {
    let cmdrun = std::process::Command::new("loginctl")
        .arg("list-sessions")
        .arg("--output=json")
        .output()
        .context("failed to run `loginctl` binary")?;

    if !cmdrun.status.success() {
        bail!(
            "`loginctl` failed to list current sessions: {}",
            String::from_utf8_lossy(&cmdrun.stderr)
        );
    }

    let sessions = serde_json::from_slice(&cmdrun.stdout)
        .context("failed to deserialize output of `loginctl`")?;

    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpm_ostree::Release;
    use std::{thread, time};

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

        let state_change_time_before = LATEST_STATE_CHANGE.get();
        thread::sleep(time::Duration::from_secs(1));
        machine.no_new_update(); // ReportedSteady to NoNewUpdate.
        let state_change_time_after = LATEST_STATE_CHANGE.get();
        assert_eq!(machine, UpdateAgentState::NoNewUpdate);
        assert_ne!(state_change_time_before, state_change_time_after);

        let state_change_time_before = LATEST_STATE_CHANGE.get();
        thread::sleep(time::Duration::from_secs(1));
        machine.no_new_update(); // NoNewUpdate to NoNewUpdate.
        let state_change_time_after = LATEST_STATE_CHANGE.get();
        assert_eq!(machine, UpdateAgentState::NoNewUpdate);
        // Transitioning to own state not considered state change.
        assert_eq!(state_change_time_before, state_change_time_after);

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

        let (persistent_err, _) = machine.record_failed_deploy();
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
            let (persistent_err, _) = machine.record_failed_deploy();
            assert_eq!(persistent_err, false);
            assert_eq!(
                machine,
                UpdateAgentState::UpdateAvailable((update.clone(), attempt as u8))
            )
        }

        // Persistent error threshold reached.
        let (persistent_err, _) = machine.record_failed_deploy();
        assert_eq!(persistent_err, true);
        assert_eq!(machine, UpdateAgentState::NoNewUpdate);
    }
}
