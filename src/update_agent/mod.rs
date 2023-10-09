//! Update agent.

mod actor;
pub use actor::LastRefresh;

use crate::cincinnati::Cincinnati;
use crate::config::Settings;
use crate::identity::Identity;
use crate::rpm_ostree::{Release, RpmOstreeClient};
use crate::strategy::UpdateStrategy;
use actix::Addr;
use anyhow::{Context, Result};
use chrono::prelude::*;
use prometheus::{IntCounter, IntGauge};
use serde::{Deserialize, Deserializer};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Default refresh interval for steady state (in seconds).
pub(crate) const DEFAULT_STEADY_INTERVAL_SECS: u64 = 300; // 5 minutes.

/// Default refresh interval for end state (in seconds).
const END_INTERVAL_SECS: u64 = 10800; // 3 hours.

/// Default tick/refresh period for the state machine (in seconds).
const DEFAULT_REFRESH_PERIOD_SECS: u64 = 300; // 5 minutes.

/// Default amount of time to postpone finalizing an update if active
/// interactive user sessions detected.
const DEFAULT_POSTPONEMENT_TIME_SECS: u64 = 60; // 1 minute.

/// Maximum failed deploy attempts in a row in `UpdateAvailable` state
/// before abandoning a target update.
const MAX_DEPLOY_ATTEMPTS: u8 = 12;

/// Maximum number of postponements to finalizing an update in the
/// `UpdateStaged` state before forcing an update finalization and reboot.
pub(crate) const MAX_FINALIZE_POSTPONEMENTS: u8 = 10;

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
    static ref POSTPONED_FINALIZATIONS: IntCounter = register_int_counter!(opts!(
        "zincati_update_agent_postponed_finalizations_total",
        "Total number of update finalization postponements due to active users."
    )).unwrap();
    static ref DETECTED_ACTIVE_USERS: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_finalization_detected_active_users",
        "Number of active users detected by the update-agent."
    )).unwrap();
}

/// JSON output from `loginctl list-sessions --output=json`.
#[derive(Debug, Deserialize)]
pub struct SessionJson {
    user: String,
    #[serde(deserialize_with = "deserialize_systemd_tty_canonicalized")]
    tty: Option<String>,
}

/// A user login session with a tty.
pub struct InteractiveSession {
    user: String,
    /// Device file of session's tty.
    tty_dev: String,
}

/// Function to deserialize field to `Option<String>`, where empty strings or
/// `n/a` (not applicable) strings are deserialized into `None`. In systemd v254+
/// loginctl list-sessions --json started outputting `n/a` instead of an empty
/// string for tty.
fn deserialize_systemd_tty_canonicalized<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() || s == "n/a" {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

/// State machine for the agent.
#[derive(Clone, Debug, PartialEq, Eq)]
enum UpdateAgentMachineState {
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
    ///
    /// The integer counter keeps track of how many more finalization
    /// postponements are permitted. If the counter reaches zero, the
    /// finalization will proceed, disregarding any users logged in.
    /// The counter is reset to `MAX_FINALIZE_POSTPONEMENTS` if a
    /// finalization attempt failed due to update strategy constraints.
    UpdateStaged((Release, u8)),
    /// Update finalized by rpm-ostree.
    UpdateFinalized(Release),
    /// Final state upon actor end.
    EndState,
}

impl Default for UpdateAgentMachineState {
    fn default() -> Self {
        let start_state = UpdateAgentMachineState::StartState;
        LATEST_STATE_CHANGE.set(chrono::Utc::now().timestamp());
        start_state
    }
}

impl UpdateAgentMachineState {
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
        let target = UpdateAgentMachineState::Initialized;
        // Allowed starting states.
        assert!(
            *self == UpdateAgentMachineState::StartState,
            "transition not allowed: {:?} to {:?}",
            self,
            target,
        );

        self.transition_to(target);
    }

    /// Transition to the ReportedSteady state.
    fn reported_steady(&mut self) {
        let target = UpdateAgentMachineState::ReportedSteady;
        // Allowed starting states.
        assert!(
            *self == UpdateAgentMachineState::Initialized,
            "transition not allowed: {:?} to {:?}",
            self,
            target,
        );

        self.transition_to(target);
    }

    /// Transition to the NoNewUpdate state.
    fn no_new_update(&mut self) {
        let target = UpdateAgentMachineState::NoNewUpdate;
        // Allowed starting states.
        assert!(
            *self == UpdateAgentMachineState::ReportedSteady
                || *self == UpdateAgentMachineState::NoNewUpdate,
            "transition not allowed: {:?} to {:?}",
            self,
            target
        );

        self.transition_to(UpdateAgentMachineState::NoNewUpdate);
    }

    /// Transition to the UpdateAvailable state with a new release.
    fn update_available(&mut self, update: Release) {
        let target = UpdateAgentMachineState::UpdateAvailable((update, 0));
        // Allowed starting states.
        assert!(
            *self == UpdateAgentMachineState::ReportedSteady
                || *self == UpdateAgentMachineState::NoNewUpdate,
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
            UpdateAgentMachineState::UpdateAvailable((r, a)) => (r, a),
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
        let target = UpdateAgentMachineState::UpdateAvailable((update, fail_count));

        self.transition_to(target);
    }

    /// Transition to the NoNewUpdate state after persistent deploy failure.
    fn update_abandoned(&mut self) {
        let target = UpdateAgentMachineState::NoNewUpdate;

        self.transition_to(target);
    }

    /// Transition to the UpdateStaged state, setting the number of postponements
    /// remaining to `MAX_FINALIZE_POSTPONEMENTS`.
    fn update_staged(&mut self, update: Release) {
        let target = UpdateAgentMachineState::UpdateStaged((update, MAX_FINALIZE_POSTPONEMENTS));

        self.transition_to(target);
    }

    /// Determine whether to allow finalization based off of current state.
    /// Returns a boolean indicating whether a finalization is permitted.
    fn usersessions_can_finalize(&mut self) -> bool {
        let interactive_sessions = get_interactive_user_sessions();

        // If we failed to check for interactive sessions, assume nobody
        // is logged in. Overall, we prefer forcibly finalizing updates
        // instead of getting stuck in a retry loop here.
        let sessions = interactive_sessions.unwrap_or_else(|e| {
            log::error!("failed to check for interactive sessions: {}", e);
            log::warn!("assuming no active sessions and proceeding anyway");
            vec![]
        });

        self.handle_interactive_sessions(&sessions)
    }

    /// Helper for determining whether to allow a finalization by first checking whether
    /// interactive sessions are present and then handling the appropriate response to current
    /// state's remaining postponements (possibly broadcasting warning messages to active sessions).
    ///
    /// Returns a boolean indicating whether finalization is permitted.
    fn handle_interactive_sessions(&self, interactive_sessions: &[InteractiveSession]) -> bool {
        DETECTED_ACTIVE_USERS.set(interactive_sessions.len() as i64);
        log::trace!(
            "handling interactive sessions, total: {}",
            interactive_sessions.len()
        );

        // Happy path, nobody to wait for.
        if interactive_sessions.is_empty() {
            log::debug!("no interactive sessions detected");
            return true;
        }

        let (release, postponements_remaining) = match self {
            UpdateAgentMachineState::UpdateStaged((r, p)) => (r, *p),
            _ => unreachable!(
                "transition not allowed: handle_interactive_sessions on {:?}",
                self,
            ),
        };

        // Maximum grace period reached, not delaying any further.
        if postponements_remaining == 0 {
            log::warn!("reached end of grace period while waiting for interactive sessions");
            return true;
        }

        if postponements_remaining == MAX_FINALIZE_POSTPONEMENTS {
            let max_reboot_delay_secs =
                DEFAULT_POSTPONEMENT_TIME_SECS.saturating_mul(MAX_FINALIZE_POSTPONEMENTS as u64);
            log::warn!(
                "interactive sessions detected, entering grace period (maximum {})",
                format_seconds(max_reboot_delay_secs)
            );
            let warning_msg = format_reboot_warning(max_reboot_delay_secs, &release.version);
            broadcast(&warning_msg, interactive_sessions);
        } else if postponements_remaining == 1 {
            log::warn!("last attempt to wait for the end of all interactive sessions");
            let warning_msg =
                format_reboot_warning(DEFAULT_POSTPONEMENT_TIME_SECS, &release.version);
            broadcast(&warning_msg, interactive_sessions);
        }

        false
    }

    /// Record an additional postponement in machine's state (reduce the number of remaining
    /// postponements allowed by one) after a finalization postponement.
    fn record_postponement(&mut self) {
        let (release, postponements_remaining) = match self.clone() {
            UpdateAgentMachineState::UpdateStaged((r, p)) => (r, p),
            _ => unreachable!(
                "transition not allowed: handle_interactive_sessions on {:?}",
                self,
            ),
        };

        POSTPONED_FINALIZATIONS.inc();
        self.reboot_postponed(release, postponements_remaining.saturating_sub(1));
    }

    /// Transition to the UpdateStaged state, setting the number of postponements
    /// remaining to postponements_remaining.
    fn reboot_postponed(&mut self, update: Release, postponements_remaining: u8) {
        let target = UpdateAgentMachineState::UpdateStaged((update, postponements_remaining));

        self.transition_to(target);
    }

    /// Transition to the UpdateFinalized state.
    fn update_finalized(&mut self, update: Release) {
        let target = UpdateAgentMachineState::UpdateFinalized(update);

        self.transition_to(target);
    }

    /// Transition to the End state.
    fn end(&mut self) {
        let target = UpdateAgentMachineState::EndState;

        self.transition_to(target);
    }

    /// Return the amount of delay between refreshes for this state, and whether
    /// jitter should be added.
    fn get_refresh_delay(&self, steady_interval: Duration) -> (Duration, bool) {
        match self {
            UpdateAgentMachineState::ReportedSteady | UpdateAgentMachineState::NoNewUpdate => {
                (steady_interval, true)
            }
            UpdateAgentMachineState::UpdateStaged((_, postponements)) => {
                // If postponements is less than `MAX_FINALIZE_POSTPONEMENTS`, that means the current tick
                // led to a postponment, and so we should add a delay of `DEFAULT_POSTPONEMENT_TIME_SECS`.
                if *postponements < MAX_FINALIZE_POSTPONEMENTS {
                    (Duration::from_secs(DEFAULT_POSTPONEMENT_TIME_SECS), false)
                } else {
                    (Duration::from_secs(DEFAULT_REFRESH_PERIOD_SECS), true)
                }
            }
            UpdateAgentMachineState::EndState => (Duration::from_secs(END_INTERVAL_SECS), true),
            _ => (Duration::from_secs(DEFAULT_REFRESH_PERIOD_SECS), true),
        }
    }
}

/// Update agent.
#[derive(Debug)]
pub(crate) struct UpdateAgent {
    /// Current state of the agent.
    /// We use an `Rc` because consumers of this field will likely need to
    /// own it (e.g. consumers in futures).
    state: Rc<RwLock<UpdateAgentState>>,
    /// Timestamp of last state transition.
    /// Behind `Rc` for same reason as above.
    /// `Cell` is sufficient because the update_agent actor runs on a single thread (SystemArbiter).
    state_changed: Rc<Cell<DateTime<Utc>>>,
    /// Update agent's information.
    info: UpdateAgentInfo,
}

/// Non-atomic read-write state of the update agent.
#[derive(Debug, Default)]
pub(crate) struct UpdateAgentState {
    /// Current state of the agent state machine.
    machine_state: UpdateAgentMachineState,
    /// List of releases to ignore.
    denylist: BTreeSet<Release>,
}

/// Read-only information about the update agent.
#[derive(Debug, Clone)]
pub(crate) struct UpdateAgentInfo {
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
}

impl UpdateAgent {
    /// Build an update agent with the given config.
    pub(crate) fn with_config(cfg: Settings, rpm_ostree_addr: Addr<RpmOstreeClient>) -> Self {
        let steady_secs = cfg.steady_interval_secs.get();
        Self {
            state: Rc::new(RwLock::new(UpdateAgentState::default())),
            state_changed: Rc::new(Cell::new(chrono::Utc::now())),
            info: UpdateAgentInfo {
                allow_downgrade: cfg.allow_downgrade,
                cincinnati: cfg.cincinnati,
                enabled: cfg.enabled,
                identity: cfg.identity,
                rpm_ostree_actor: rpm_ostree_addr,
                steady_interval: Duration::from_secs(steady_secs),
                strategy: cfg.strategy,
            },
        }
    }
}

/// Attempt to broadcast msg to sessions.
fn broadcast(msg: &str, sessions: &[InteractiveSession]) {
    let mut sessions_broadcasted: usize = 0;

    let broadcast_msg = format!(
        "\nBroadcast message from Zincati at {}:\n{}\n",
        chrono::Utc::now().format("%a %Y-%m-%d %H:%M:%S %Z"),
        msg
    );

    for session in sessions.iter() {
        // Write message to tty device.
        log::trace!(
            "Attempting to broadcast a message to user {} at {}",
            &session.user,
            &session.tty_dev
        );
        if let Err(e) = fs::write(&session.tty_dev, &broadcast_msg) {
            log::error!("failed to write to {}: {}", &session.tty_dev, e);
            continue;
        };

        sessions_broadcasted = sessions_broadcasted.saturating_add(1);
    }

    if sessions.len() != sessions_broadcasted {
        log::warn!(
            "{} interactive sessions found, but only broadcasted to {}",
            sessions.len(),
            sessions_broadcasted
        );
    }
}

/// Get sessions with logged in interactive users using `loginctl`.
/// Returns a Result with vector of `SessionsJson` if no error.
fn get_interactive_user_sessions() -> Result<Vec<InteractiveSession>> {
    let cmdrun = std::process::Command::new("loginctl")
        .arg("list-sessions")
        .arg("--output=json")
        .output()
        .context("failed to run `loginctl` binary")?;

    if !cmdrun.status.success() {
        anyhow::bail!(
            "`loginctl` failed to list current sessions: {}",
            String::from_utf8_lossy(&cmdrun.stderr)
        );
    }

    let sessions: Vec<SessionJson> = serde_json::from_slice(&cmdrun.stdout)
        .context("failed to deserialize output of `loginctl`")?;

    // Filter out sessions that aren't interactive (don't have a tty), and map
    // these sessions into an `InteractiveSession` struct.
    let interactive_session: Vec<InteractiveSession> = sessions
        .into_iter()
        .filter_map(|session| match session.tty {
            Some(mut tty) => {
                tty.insert_str(0, "/dev/");
                Some(InteractiveSession {
                    user: session.user,
                    tty_dev: tty,
                })
            }
            _ => {
                log::debug!(
                    "found user {} with no tty, user considered non-interactive",
                    session.user
                );
                None
            }
        })
        .collect();

    Ok(interactive_session)
}

/// Returns a warning string about the time until reboot and the release
/// that is staged.
fn format_reboot_warning(seconds: u64, release_ver: &str) -> String {
    let time_till_reboot = format_seconds(seconds);

    format!(
        "New update {} is available and has been deployed.\n\
        If permitted by the update strategy, Zincati will reboot into this update when\n\
        all interactive users have logged out, or in {}, whichever comes\n\
        earlier. Please log out of all active sessions in order to let the auto-update\n\
        process continue.",
        release_ver, time_till_reboot
    )
}

/// Helper to return a human-friendly version of seconds.
/// Example: 65 seconds would be converted to 1 minute and 5 seconds.
fn format_seconds(seconds: u64) -> String {
    let mut time_till_reboot = if seconds / 60 >= 1 {
        format!(
            "{} minute{}{}",
            seconds / 60,
            if seconds / 60 == 1 { "" } else { "s" },
            if seconds % 60 > 0 { " and " } else { "" }
        )
    } else {
        String::from("")
    };
    if seconds % 60 > 0 {
        time_till_reboot.push_str(&format!(
            "{} second{}",
            seconds % 60,
            if seconds % 60 == 1 { "" } else { "s" }
        ))
    }

    time_till_reboot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpm_ostree::Release;
    use std::{thread, time};

    #[test]
    fn default_state() {
        assert_eq!(
            UpdateAgentMachineState::default(),
            UpdateAgentMachineState::StartState
        );
    }

    #[test]
    fn state_machine_happy_path() {
        let steady_interval = Duration::from_secs(DEFAULT_STEADY_INTERVAL_SECS);
        let default_interval = Duration::from_secs(DEFAULT_REFRESH_PERIOD_SECS);

        let mut machine = UpdateAgentMachineState::default();
        assert_eq!(machine, UpdateAgentMachineState::StartState);

        machine.initialized();
        assert_eq!(machine, UpdateAgentMachineState::Initialized);

        machine.reported_steady();
        assert_eq!(machine, UpdateAgentMachineState::ReportedSteady);

        let state_change_time_before = LATEST_STATE_CHANGE.get();
        thread::sleep(time::Duration::from_secs(1));
        machine.no_new_update(); // ReportedSteady to NoNewUpdate.
        let state_change_time_after = LATEST_STATE_CHANGE.get();
        assert_eq!(machine, UpdateAgentMachineState::NoNewUpdate);
        assert_ne!(state_change_time_before, state_change_time_after);
        let (delay, should_jitter) = machine.get_refresh_delay(steady_interval);
        assert_eq!(delay, steady_interval);
        assert!(should_jitter);

        let state_change_time_before = LATEST_STATE_CHANGE.get();
        thread::sleep(time::Duration::from_secs(1));
        machine.no_new_update(); // NoNewUpdate to NoNewUpdate.
        let state_change_time_after = LATEST_STATE_CHANGE.get();
        assert_eq!(machine, UpdateAgentMachineState::NoNewUpdate);
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
            UpdateAgentMachineState::UpdateAvailable((update.clone(), 0))
        );

        let (persistent_err, _) = machine.record_failed_deploy();
        assert!(!persistent_err);
        assert_eq!(
            machine,
            UpdateAgentMachineState::UpdateAvailable((update.clone(), 1))
        );
        let (delay, should_jitter) = machine.get_refresh_delay(steady_interval);
        assert_eq!(delay, default_interval);
        assert!(should_jitter);

        machine.update_staged(update.clone());
        assert_eq!(
            machine,
            UpdateAgentMachineState::UpdateStaged((update.clone(), MAX_FINALIZE_POSTPONEMENTS))
        );

        machine.update_finalized(update.clone());
        assert_eq!(machine, UpdateAgentMachineState::UpdateFinalized(update));

        machine.end();
        assert_eq!(machine, UpdateAgentMachineState::EndState);
    }

    #[test]
    fn test_fsm_abandon_update() {
        let update = Release {
            version: "v1".to_string(),
            checksum: "ostree-checksum".to_string(),
            age_index: None,
        };
        let mut machine = UpdateAgentMachineState::NoNewUpdate;

        machine.update_available(update.clone());
        assert_eq!(
            machine,
            UpdateAgentMachineState::UpdateAvailable((update.clone(), 0))
        );

        // MAX-1 temporary failures.
        for attempt in 1..MAX_DEPLOY_ATTEMPTS {
            let (persistent_err, _) = machine.record_failed_deploy();
            assert!(!persistent_err);
            assert_eq!(
                machine,
                UpdateAgentMachineState::UpdateAvailable((update.clone(), attempt))
            )
        }

        // Persistent error threshold reached.
        let (persistent_err, _) = machine.record_failed_deploy();
        assert!(persistent_err);
        assert_eq!(machine, UpdateAgentMachineState::NoNewUpdate);
    }

    #[test]
    fn test_fsm_postpone_finalize() {
        let steady_interval = Duration::from_secs(DEFAULT_STEADY_INTERVAL_SECS);
        let default_interval = Duration::from_secs(DEFAULT_REFRESH_PERIOD_SECS);
        let postponement_interval = Duration::from_secs(DEFAULT_POSTPONEMENT_TIME_SECS);
        let update = Release {
            version: "v1".to_string(),
            checksum: "ostree-checksum".to_string(),
            age_index: None,
        };
        let mut machine = UpdateAgentMachineState::UpdateAvailable((update.clone(), 0));
        let (delay, should_jitter) = machine.get_refresh_delay(steady_interval);
        assert_eq!(delay, default_interval);
        assert!(should_jitter);

        machine.update_staged(update.clone());
        assert_eq!(
            machine,
            UpdateAgentMachineState::UpdateStaged((update.clone(), MAX_FINALIZE_POSTPONEMENTS))
        );

        // Set up empty interactive sessions.
        let no_interactive_sessions: Vec<InteractiveSession> = vec![];
        let can_finalize = machine.handle_interactive_sessions(&no_interactive_sessions);
        assert!(can_finalize);
        assert_eq!(
            machine,
            UpdateAgentMachineState::UpdateStaged((update.clone(), MAX_FINALIZE_POSTPONEMENTS))
        );
        let (delay, should_jitter) = machine.get_refresh_delay(steady_interval);
        assert_eq!(delay, default_interval);
        assert!(should_jitter);

        // Set up dummy interactive sessions.
        let fake_tty_path = tempfile::tempdir_in("/tmp").unwrap();
        let fake_tty_path_str = fake_tty_path.path().to_str().unwrap();
        let fake_tty = format!("{}/tty1", fake_tty_path_str);
        let fake_session = InteractiveSession {
            user: String::from("fakeuser"),
            tty_dev: String::from(&fake_tty),
        };
        let interactive_sessions_present: Vec<InteractiveSession> = vec![fake_session];

        // Postpone MAX_FINALIZE_POSTPONEMENTS times (counting from 1).
        for finalization_attempt in 1..MAX_FINALIZE_POSTPONEMENTS + 1 {
            let can_finalize = machine.handle_interactive_sessions(&interactive_sessions_present);
            assert!(!can_finalize);
            machine.record_postponement(); // as we cannot finalize.
            let postponement_remaining =
                MAX_FINALIZE_POSTPONEMENTS.saturating_sub(finalization_attempt);
            assert_eq!(
                machine,
                UpdateAgentMachineState::UpdateStaged((update.clone(), postponement_remaining))
            );
            let (delay, should_jitter) = machine.get_refresh_delay(steady_interval);
            assert_eq!(delay, postponement_interval);
            assert!(!should_jitter);
        }

        // Sanity check final broadcasted message.
        let tty_contents = fs::read_to_string(&fake_tty).unwrap();
        assert!(tty_contents.contains("Broadcast message from Zincati"));
        assert!(tty_contents.contains(&update.version));
        assert!(tty_contents.contains(&format_seconds(DEFAULT_POSTPONEMENT_TIME_SECS)));

        // Reached 0 remaining postponements.
        let can_finalize = machine.handle_interactive_sessions(&interactive_sessions_present);
        assert!(can_finalize);
        assert_eq!(machine, UpdateAgentMachineState::UpdateStaged((update, 0)));
    }

    #[test]
    fn test_format_seconds() {
        assert_eq!("1 second", format_seconds(1));
        assert_eq!("2 seconds", format_seconds(2));
        assert_eq!("1 minute", format_seconds(60));
        assert_eq!("1 minute and 1 second", format_seconds(60 + 1));
        assert_eq!("1 minute and 30 seconds", format_seconds(60 + 30));
        assert_eq!("2 minutes", format_seconds(2 * 60));
        assert_eq!("42 minutes and 23 seconds", format_seconds(42 * 60 + 23));
    }
}
