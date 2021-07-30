//! Update agent actor.

use super::{UpdateAgent, UpdateAgentInfo, UpdateAgentState};
use crate::rpm_ostree::{self, Release};
use crate::utils;
use actix::prelude::*;
use anyhow::{Error, Result};
use futures::prelude::*;
use log::trace;
use prometheus::{IntCounter, IntCounterVec, IntGauge};
use std::collections::BTreeSet;
use std::mem::discriminant;
use std::rc::Rc;
use std::time::Duration;

/// Label for finalization attempts blocked due to active interactive user sessions.
pub static ACTIVE_USERSESSIONS_LABEL: &str = "active_usersessions";

lazy_static::lazy_static! {
    static ref LAST_REFRESH: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_last_refresh_timestamp",
        "UTC timestamp of update-agent last refresh tick."
    )).unwrap();
    static ref FINALIZATION_ATTEMPTS: IntCounter = register_int_counter!(opts!(
        "zincati_update_agent_finalization_attempts",
        "Total number of attempts to finalize a staged deployment by the update agent."
    )).unwrap();
    static ref FINALIZATION_BLOCKED: IntCounterVec = register_int_counter_vec!(
        "zincati_update_agent_finalization_blocked_count",
        "Total number of finalization attempts blocked due to reasons unrelated to update strategy.",
        &["reason"]
    ).unwrap();
    static ref FINALIZATION_SUCCESS: IntCounter = register_int_counter!(opts!(
        "zincati_update_agent_finalization_successes",
        "Total number of successful update finalizations by the update agent."
    )).unwrap();
}

impl Actor for UpdateAgent {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        trace!("update agent started");

        if self.info.allow_downgrade {
            log::warn!("client configuration allows (possibly vulnerable) downgrades via auto-updates logic");
        }

        // Kick-start the state machine.
        Self::tick_now(ctx);
    }
}

pub struct LastRefresh {}

impl Message for LastRefresh {
    type Result = i64;
}

impl Handler<LastRefresh> for UpdateAgent {
    type Result = i64;

    fn handle(&mut self, _msg: LastRefresh, _ctx: &mut Self::Context) -> Self::Result {
        trace!("agent: request to get last refresh time");
        LAST_REFRESH.get()
    }
}

pub(crate) struct RefreshTick {}

impl Message for RefreshTick {
    type Result = Result<(), Error>;
}

impl Handler<RefreshTick> for UpdateAgent {
    type Result = ResponseActFuture<Self, Result<(), Error>>;

    fn handle(&mut self, _msg: RefreshTick, _ctx: &mut Self::Context) -> Self::Result {
        // We need a clone of `info` because we need to move it into futures to ensure a
        // long enough lifetime.
        let update_agent_info = self.info.clone();
        let lock = Rc::clone(&self.state);
        let last_changed = Rc::clone(&self.state_changed);
        let state_action = async move {
            // Acquire RwLock to access state.
            let mut agent_state_guard = lock.write().await;
            // Consider `LAST_REFRESH` time to be when lock is acquired.
            let tick_timestamp = chrono::Utc::now();
            LAST_REFRESH.set(tick_timestamp.timestamp());

            trace!("update agent tick, current state: {:?}", *agent_state_guard);
            let prev_state = (*agent_state_guard).clone();

            match &prev_state {
                UpdateAgentState::StartState => {
                    update_agent_info
                        .tick_initialize(&mut *agent_state_guard)
                        .await
                }
                UpdateAgentState::Initialized => {
                    update_agent_info
                        .tick_report_steady(&mut *agent_state_guard)
                        .await
                }
                UpdateAgentState::ReportedSteady => {
                    update_agent_info
                        .tick_check_updates(&mut *agent_state_guard)
                        .await
                }
                UpdateAgentState::NoNewUpdate => {
                    update_agent_info
                        .tick_check_updates(&mut *agent_state_guard)
                        .await
                }
                UpdateAgentState::UpdateAvailable((release, _)) => {
                    let update = release.clone();
                    update_agent_info
                        .tick_stage_update(&mut *agent_state_guard, update)
                        .await
                }
                UpdateAgentState::UpdateStaged((release, _)) => {
                    let update = release.clone();
                    update_agent_info
                        .tick_finalize_update(&mut *agent_state_guard, update)
                        .await
                }
                UpdateAgentState::UpdateFinalized(release) => {
                    let update = release.clone();
                    update_agent_info
                        .tick_end(&mut *agent_state_guard, update)
                        .await
                }
                UpdateAgentState::EndState => (),
            };

            // Update state_changed timestamp if necessary.
            if discriminant(&prev_state) != discriminant(&*agent_state_guard) {
                let cur_timestamp = chrono::Utc::now();
                // This mutable borrow will not panic because we are still holding
                // the UpdateAgentState's `RwLock` for writing.
                *last_changed.borrow_mut() = cur_timestamp;
            }

            Self::refresh_delay(
                update_agent_info.steady_interval,
                &prev_state,
                &*agent_state_guard,
            )
        };
        let state_action = state_action.into_actor(self);
        let update_machine = state_action.then(|pause, _actor, ctx| {
            if let Some(pause) = pause {
                log::trace!(
                    "scheduling next agent refresh in {} seconds",
                    pause.as_secs()
                );
                Self::tick_later(ctx, pause);
            } else {
                Self::tick_now(ctx);
            }
            actix::fut::ok(())
        });

        Box::pin(update_machine)
    }
}

impl UpdateAgent {
    /// Schedule an immediate refresh of the state machine.
    pub fn tick_now(ctx: &mut Context<Self>) {
        ctx.notify(RefreshTick {})
    }

    /// Schedule a delayed refresh of the state machine.
    pub fn tick_later(ctx: &mut Context<Self>, after: std::time::Duration) -> actix::SpawnHandle {
        ctx.notify_later(RefreshTick {}, after)
    }

    /// Pausing interval between state-machine refresh cycles.
    ///
    /// This influences the pace of the update-agent refresh loop. Timing of the
    /// state machine is not uniform. Some states benefit from more/less
    /// frequent refreshes, or can be customized by the user.
    fn refresh_delay(
        steady_interval: Duration,
        prev_state: &UpdateAgentState,
        cur_state: &UpdateAgentState,
    ) -> Option<Duration> {
        if Self::should_tick_immediately(prev_state, cur_state) {
            return None;
        }

        let (mut refresh_delay, should_jitter) = cur_state.get_refresh_delay(steady_interval);
        if should_jitter {
            refresh_delay = Self::add_jitter(refresh_delay);
        };

        Some(refresh_delay)
    }

    /// Return whether a transition from `prev_state` to `cur_state` warrants an immediate
    /// tick.
    fn should_tick_immediately(
        prev_state: &UpdateAgentState,
        cur_state: &UpdateAgentState,
    ) -> bool {
        // State changes trigger immediate tick/action.
        if discriminant(prev_state) != discriminant(cur_state) {
            // Unless we're transitioning from ReportedSteady to NoNewUpdate.
            if !(*prev_state == UpdateAgentState::ReportedSteady
                && *cur_state == UpdateAgentState::NoNewUpdate)
            {
                return true;
            }
        }
        false
    }

    /// Add a small, random amount (0% to 10%) of jitter to a given period.
    ///
    /// This random jitter is useful to prevent clients from converging to
    /// the same phase-locked loop.
    fn add_jitter(period: std::time::Duration) -> std::time::Duration {
        use rand::Rng;

        let secs = period.as_secs();
        let rand: u8 = rand::thread_rng().gen_range(0..=10);
        let jitter = u64::max(secs / 100, 1).saturating_mul(u64::from(rand));
        std::time::Duration::from_secs(secs.saturating_add(jitter))
    }

    /// Log at INFO level how many and which deployments will be excluded from being
    /// future update targets.
    fn log_excluded_depls(depls: &BTreeSet<Release>, actor: &UpdateAgentInfo) {
        // Exclude booted deployment.
        let mut other_depls = depls.clone();
        if !other_depls.remove(&actor.identity.current_os) {
            log::error!("could not find booted deployment in deployments");
            return; // Early return since this really should not happen.
        }

        let excluded_depls_count = other_depls.len();
        if excluded_depls_count > 0 {
            log::info!(
                "found {} other finalized deployment{}",
                excluded_depls_count,
                if excluded_depls_count > 1 { "s" } else { "" }
            );
            for release in other_depls {
                log::info!(
                    "deployment {} ({}) will be excluded from being a future update target",
                    release.version,
                    release.checksum
                );
            }
        } else {
            log::debug!(
                "no other local finalized deployments found; no update targets will be excluded."
            );
        }
    }
}

impl UpdateAgentInfo {
    /// Initialize the update agent.
    async fn tick_initialize(&self, state: &mut UpdateAgentState) {
        trace!("update agent in start state");
        if self.enabled {
            self.register_as_driver().await;
        }
        let local_depls = self.local_deployments().await;
        match local_depls {
            Ok(depls) => UpdateAgent::log_excluded_depls(&depls, &self),
            Err(e) => log::error!("failed to query local deployments: {}", e),
        }
        let status;
        if self.enabled {
            status = "initialization complete, auto-updates logic enabled";
            log::info!("{}", status);
            state.initialized();
            self.strategy.record_details();
        } else {
            status = "initialization complete, auto-updates logic disabled by configuration";
            log::warn!("{}", status);
            state.end();
        }

        utils::notify_ready();
        utils::update_unit_status(status);
    }

    /// Try to report steady state.
    async fn tick_report_steady(&self, state: &mut UpdateAgentState) {
        trace!("trying to report steady state");

        let is_steady = self.strategy.report_steady().await;
        if is_steady {
            log::info!("reached steady state, periodically polling for updates");
            utils::update_unit_status("periodically polling for updates");
            state.reported_steady();
        }
    }

    /// Try to check for updates.
    async fn tick_check_updates(&self, state: &mut UpdateAgentState) {
        trace!("trying to check for udpates");

        let local_depls = self.local_deployments().await;
        let timestamp_now = chrono::Utc::now();
        utils::update_unit_status(&format!(
            "periodically polling for updates (last checked {})",
            timestamp_now.format("%a %Y-%m-%d %H:%M:%S %Z")
        ));
        let allow_downgrade = self.allow_downgrade;

        let release = match local_depls {
            Ok(depls) => {
                self.cincinnati
                    .fetch_update_hint(&self.identity, depls, allow_downgrade)
                    .await
            }
            Err(e) => {
                log::error!("failed to query local deployments: {}", e);
                None
            }
        };

        match release {
            Some(release) => {
                utils::update_unit_status(&format!("found update on remote: {}", release.version));
                state.update_available(release);
            }
            None => {
                state.no_new_update();
            }
        }
    }

    /// Try to stage an update.
    async fn tick_stage_update(&self, mut state: &mut UpdateAgentState, release: Release) {
        trace!("trying to stage an update");

        let target = release.clone();
        let deploy_outcome = self.attempt_deploy(target).await;

        match deploy_outcome {
            Ok(release) => {
                let msg = format!("update staged: {}", release.version);
                utils::update_unit_status(&msg);
                log::info!("{}", msg);
                state.update_staged(release);
            }
            Err(e) => {
                log::error!("failed to stage deployment: {}", e);
                let release_ver = release.version.clone();
                let fail_count = UpdateAgentInfo::deploy_attempt_failed(release, &mut state);
                let msg = format!(
                    "trying to stage {} ({} failed deployment attempt{})",
                    release_ver,
                    fail_count,
                    if fail_count > 1 { "s" } else { "" }
                );
                utils::update_unit_status(&msg);
                log::trace!("{}", msg);
            }
        };
    }

    /// Try to finalize an update.
    async fn tick_finalize_update(&self, state: &mut UpdateAgentState, release: Release) {
        trace!("trying to finalize an update");
        FINALIZATION_ATTEMPTS.inc();

        let strategy_can_finalize = self.strategy.can_finalize().await;
        if !strategy_can_finalize {
            utils::update_unit_status(&format!(
                "update staged: {}; reboot pending due to update strategy",
                &release.version
            ));
            // Reset number of postponements to `MAX_FINALIZE_POSTPONEMENTS`
            // if strategy does not allow finalization.
            state.update_staged(release);
            return;
        }

        let usersessions_can_finalize = state.usersessions_can_finalize();
        if !usersessions_can_finalize {
            FINALIZATION_BLOCKED
                .with_label_values(&[ACTIVE_USERSESSIONS_LABEL])
                .inc();
            utils::update_unit_status(&format!(
                "update staged: {}; reboot delayed due to active user sessions",
                release.version
            ));
            // Record postponement and postpone finalization.
            state.record_postponement();
            return;
        }

        match self.finalize_deployment(release).await {
            Ok(release) => {
                FINALIZATION_SUCCESS.inc();
                utils::update_unit_status(&format!("update finalized: {}", release.version));
                state.update_finalized(release);
            }
            Err(e) => log::error!("failed to finalize deployment: {}", e),
        }
    }

    /// Actor job is done.
    async fn tick_end(&self, state: &mut UpdateAgentState, release: Release) {
        let status = format!("update applied, waiting for reboot: {}", release.version);
        log::info!("{}", status);
        state.end();
        utils::update_unit_status(&status);
    }

    /// Fetch and stage an update, in finalization-locked mode.
    async fn attempt_deploy(&self, release: Release) -> Result<Release> {
        log::info!(
            "target release '{}' selected, proceeding to stage it",
            release.version
        );
        let msg = rpm_ostree::StageDeployment {
            release,
            allow_downgrade: self.allow_downgrade,
        };
        let upgrade = self
            .rpm_ostree_actor
            .send(msg)
            .unwrap_or_else(|e| Err(e.into()))
            .await;

        upgrade
    }

    /// Record a failed deploy attempt and return the total number of
    /// failed deployment attempts.
    fn deploy_attempt_failed(release: Release, state: &mut UpdateAgentState) -> u8 {
        let (is_abandoned, fail_count) = state.record_failed_deploy();
        if is_abandoned {
            log::warn!(
                "persistent deploy failure detected, target release '{}' abandoned",
                release.version
            );
        }
        fail_count
    }

    /// List persistent (i.e. finalized) local deployments.
    ///
    /// This ignores deployments that have been only staged but not finalized in the
    /// past, as they are acceptable as future update target.
    async fn local_deployments(&self) -> Result<BTreeSet<Release>> {
        let msg = rpm_ostree::QueryLocalDeployments { omit_staged: true };
        let depls = self
            .rpm_ostree_actor
            .send(msg)
            .unwrap_or_else(|e| Err(e.into()))
            .map_ok(move |depls| {
                log::trace!("found {} local deployments", depls.len());
                depls
            })
            .await;

        depls
    }

    /// Finalize a deployment (unlock and reboot).
    async fn finalize_deployment(&self, release: Release) -> Result<Release> {
        log::info!(
            "staged deployment '{}' available, proceeding to finalize it",
            release.version
        );

        let msg = rpm_ostree::FinalizeDeployment { release };
        let upgrade = self
            .rpm_ostree_actor
            .send(msg)
            .unwrap_or_else(|e| Err(e.into()))
            .await;

        upgrade
    }

    /// Attempt to register as the update driver for rpm-ostree.
    async fn register_as_driver(&self) {
        log::info!("registering as the update driver for rpm-ostree");

        let msg = rpm_ostree::RegisterAsDriver {};
        self.rpm_ostree_actor
            .send(msg)
            .unwrap_or_else(|e| log::error!("failed to register as driver: {}", e))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_tick_immediately() {
        use crate::update_agent::MAX_FINALIZE_POSTPONEMENTS;

        // Dummy `Release`.
        let update = Release {
            version: "v1".to_string(),
            checksum: "ostree-checksum".to_string(),
            age_index: None,
        };

        // Transition between states with different discriminants.
        let prev_state = UpdateAgentState::Initialized;
        let cur_state = UpdateAgentState::ReportedSteady;
        assert!(UpdateAgent::should_tick_immediately(
            &prev_state,
            &cur_state
        ));
        let prev_state = UpdateAgentState::NoNewUpdate;
        let cur_state = UpdateAgentState::UpdateAvailable((update.clone(), 0));
        assert!(UpdateAgent::should_tick_immediately(
            &prev_state,
            &cur_state
        ));
        // Note we do NOT expect an immediate tick as this is a special case.
        let prev_state = UpdateAgentState::ReportedSteady;
        let cur_state = UpdateAgentState::NoNewUpdate;
        assert!(!UpdateAgent::should_tick_immediately(
            &prev_state,
            &cur_state
        ));

        // Transition between states with same discriminants.
        let prev_state = UpdateAgentState::NoNewUpdate;
        let cur_state = UpdateAgentState::NoNewUpdate;
        assert!(!UpdateAgent::should_tick_immediately(
            &prev_state,
            &cur_state
        ));
        let prev_state = UpdateAgentState::UpdateAvailable((update.clone(), 0));
        let cur_state = UpdateAgentState::UpdateAvailable((update.clone(), 1));
        assert!(!UpdateAgent::should_tick_immediately(
            &prev_state,
            &cur_state
        ));
        let prev_state =
            UpdateAgentState::UpdateStaged((update.clone(), MAX_FINALIZE_POSTPONEMENTS));
        let cur_state = UpdateAgentState::UpdateStaged((
            update.clone(),
            MAX_FINALIZE_POSTPONEMENTS.saturating_sub(1),
        ));
        assert!(!UpdateAgent::should_tick_immediately(
            &prev_state,
            &cur_state
        ));
    }
}
