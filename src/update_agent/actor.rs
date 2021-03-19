//! Update agent actor.

use super::{UpdateAgent, UpdateAgentState};
use crate::rpm_ostree::{self, Release};
use actix::prelude::*;
use failure::Error;
use futures::prelude::*;
use libsystemd::daemon::*;
use log::trace;
use prometheus::IntGauge;
use std::collections::BTreeSet;
use std::time::Duration;

lazy_static::lazy_static! {
    static ref LAST_REFRESH: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_last_refresh_timestamp",
        "UTC timestamp of update-agent last refresh tick."
    )).unwrap();
}

impl Actor for UpdateAgent {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        trace!("update agent started");

        if self.allow_downgrade {
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

    fn handle(&mut self, _msg: RefreshTick, ctx: &mut Self::Context) -> Self::Result {
        let tick_timestamp = chrono::Utc::now();
        LAST_REFRESH.set(tick_timestamp.timestamp());

        trace!("update agent tick, current state: {:?}", self.state);
        let prev_state = self.state.clone();

        let state_action = match &self.state {
            UpdateAgentState::StartState => self.tick_initialize(),
            UpdateAgentState::Initialized => self.tick_report_steady(),
            UpdateAgentState::ReportedSteady => self.tick_check_updates(),
            UpdateAgentState::NoNewUpdate => self.tick_check_updates(),
            UpdateAgentState::UpdateAvailable((release, _)) => {
                let update = release.clone();
                self.tick_stage_update(update)
            }
            UpdateAgentState::UpdateStaged((release, _)) => {
                let update = release.clone();
                self.tick_finalize_update(update)
            }
            UpdateAgentState::UpdateFinalized(release) => {
                let update = release.clone();
                self.tick_end(update)
            }
            UpdateAgentState::EndState => self.nop(),
        };

        let update_machine = state_action.then(move |_r, actor, ctx| {
            if let Some(interval) = actor.refresh_delay(prev_state) {
                let pause = Self::add_jitter(interval);
                log::trace!(
                    "scheduling next agent refresh in {} seconds",
                    pause.as_secs()
                );
                Self::tick_later(ctx, pause);
            } else {
                let update_timestamp = chrono::Utc::now();
                actor.state_changed = update_timestamp;
                Self::tick_now(ctx);
            }
            actix::fut::ready(())
        });

        // Process state machine refresh ticks sequentially.
        ctx.wait(update_machine);

        Box::pin(actix::fut::ok(()))
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
    fn refresh_delay(&self, prev_state: UpdateAgentState) -> Option<Duration> {
        use std::mem::discriminant;

        // State changes trigger immediate tick/action.
        if discriminant(&prev_state) != discriminant(&self.state) {
            // Unless we're transitioning from ReportedSteady to NoNewUpdate.
            if !(prev_state == UpdateAgentState::ReportedSteady
                && self.state == UpdateAgentState::NoNewUpdate)
            {
                return None;
            }
        }

        let delay = match self.state {
            UpdateAgentState::ReportedSteady | UpdateAgentState::NoNewUpdate => {
                self.steady_interval
            }
            UpdateAgentState::UpdateStaged((_, postponements)) => {
                // Note: no jitter if postponing reboots.
                if postponements > 0 {
                    return Some(Duration::from_secs(super::DEFAULT_POSTPONEMENT_TIME_SECS));
                } else {
                    Duration::from_secs(super::DEFAULT_REFRESH_PERIOD_SECS)
                }
            }
            _ => Duration::from_secs(super::DEFAULT_REFRESH_PERIOD_SECS),
        };
        Some(Self::add_jitter(delay))
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
    fn log_excluded_depls(depls: &BTreeSet<Release>, actor: &UpdateAgent) {
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

    /// Initialize the update agent.
    fn tick_initialize(&mut self) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("update agent in start state");

        // Only register as the updates driver for rpm-ostree if auto-updates logic enabled.
        let initialization = if self.enabled {
            self.register_as_driver()
        } else {
            self.nop()
        }
        .then(|_r, actor, _ctx| actor.local_deployments())
        .map(|res, actor, _ctx| {
            // Report non-future update target deployments.
            if let Ok(depls) = res {
                Self::log_excluded_depls(&depls, actor);
            }
            let status;
            if actor.enabled {
                status = "initialization complete, auto-updates logic enabled";
                log::info!("{}", status);
                actor.state.initialized();
                actor.strategy.record_details();
            } else {
                status = "initialization complete, auto-updates logic disabled by configuration";
                log::warn!("{}", status);
                actor.state.end();
            };
            update_unit_status(status);
            notify_ready();
            Ok(())
        });

        Box::pin(initialization)
    }

    /// Try to report steady state.
    fn tick_report_steady(&mut self) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("trying to report steady state");

        let report_steady = self.strategy.report_steady();
        let state_change =
            actix::fut::wrap_future::<_, Self>(report_steady).map(|is_steady, actor, _ctx| {
                if is_steady {
                    log::debug!("reached steady state, periodically polling for updates");
                    update_unit_status("periodically polling for updates");
                    actor.state.reported_steady();
                }
                Ok(())
            });

        Box::pin(state_change)
    }

    /// Try to check for updates.
    fn tick_check_updates(&mut self) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("trying to check for updates");

        let state_change = self
            .local_deployments()
            .then(|res, actor, _ctx| {
                let timestamp_now = chrono::Utc::now();
                update_unit_status(&format!(
                    "periodically polling for updates (last checked {})",
                    timestamp_now.format("%a %Y-%m-%d %H:%M:%S %Z")
                ));
                let allow_downgrade = actor.allow_downgrade;
                let release = match res {
                    Ok(depls) => {
                        actor
                            .cincinnati
                            .fetch_update_hint(&actor.identity, depls, allow_downgrade)
                    }
                    _ => Box::pin(futures::future::ready(None)),
                };
                release.into_actor(actor)
            })
            .map(|res, actor, _ctx| {
                match res {
                    Some(release) => {
                        update_unit_status(&format!("found update on remote: {}", release.version));
                        actor.state.update_available(release);
                    }
                    None => {
                        actor.state.no_new_update();
                    }
                };
                Ok(())
            });

        Box::pin(state_change)
    }

    /// Try to stage an update.
    fn tick_stage_update(&mut self, release: Release) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("trying to stage an update");

        let target = release.clone();
        let deploy_outcome = self.attempt_deploy(target);
        let state_change = deploy_outcome.map(move |res, actor, _ctx| {
            match res {
                Ok(_) => {
                    update_unit_status(&format!("update staged: {}", release.version));
                    actor.state.update_staged(release);
                }
                Err(_) => {
                    let release_ver = release.version.clone();
                    let fail_count = actor.deploy_attempt_failed(release);
                    update_unit_status(&format!(
                        "trying to stage {} ({} failed deployment attempt{})",
                        release_ver,
                        fail_count,
                        if fail_count > 1 { "s" } else { "" }
                    ));
                }
            };
            Ok(())
        });

        Box::pin(state_change)
    }

    /// Try to finalize an update.
    fn tick_finalize_update(
        &mut self,
        release: Release,
    ) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("trying to finalize an update");

        let strategy_can_finalize = self.strategy.can_finalize();
        let state_change = actix::fut::wrap_future::<_, Self>(strategy_can_finalize)
            .then(|strategy_can_finalize, actor, _ctx| {
                if !strategy_can_finalize {
                    update_unit_status(&format!(
                        "update staged: {}; reboot pending due to update strategy",
                        &release.version
                    ));
                    // Reset number of postponements to `MAX_FINALIZE_POSTPONEMENTS`
                    // if strategy does not allow finalization.
                    actor.state.update_staged(release);
                    Box::pin(actix::fut::err(()))
                } else {
                    let usersessions_can_finalize = actor.state.usersessions_can_finalize();
                    if !usersessions_can_finalize {
                        update_unit_status(&format!(
                            "update staged: {}; reboot delayed due to active user sessions",
                            release.version
                        ));
                        // Record postponement and postpone finalization.
                        actor.state.record_postponement();
                        Box::pin(actix::fut::err(()))
                    } else {
                        actor.finalize_deployment(release)
                    }
                }
            })
            .map(|res, actor, _ctx| {
                res.map(|release| {
                    update_unit_status(&format!("update finalized: {}", release.version));
                    actor.state.update_finalized(release);
                })
            });

        Box::pin(state_change)
    }

    /// Actor job is done.
    fn tick_end(&mut self, release: Release) -> ResponseActFuture<Self, Result<(), ()>> {
        let status = format!("update applied, waiting for reboot: {}", release.version);
        log::info!("{}", status);
        let state_change = self.nop().map(move |_r, actor, _ctx| {
            actor.state.end();
            update_unit_status(&status);
            Ok(())
        });

        Box::pin(state_change)
    }

    /// Fetch and stage an update, in finalization-locked mode.
    fn attempt_deploy(&mut self, release: Release) -> ResponseActFuture<Self, Result<Release, ()>> {
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
            .map_err(|e| log::error!("failed to stage deployment: {}", e))
            .into_actor(self);

        Box::pin(upgrade)
    }

    /// Record a failed deploy attempt and return the total number of
    /// failed deployment attempts.
    fn deploy_attempt_failed(&mut self, release: Release) -> u8 {
        let (is_abandoned, fail_count) = self.state.record_failed_deploy();
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
    fn local_deployments(&mut self) -> ResponseActFuture<Self, Result<BTreeSet<Release>, ()>> {
        let msg = rpm_ostree::QueryLocalDeployments { omit_staged: true };
        let depls = self
            .rpm_ostree_actor
            .send(msg)
            .unwrap_or_else(|e| Err(e.into()))
            .map_err(|e| log::error!("failed to query local deployments: {}", e))
            .map_ok(move |depls| {
                log::trace!("found {} local deployments", depls.len());
                depls
            })
            .into_actor(self);

        Box::pin(depls)
    }

    /// Finalize a deployment (unlock and reboot).
    fn finalize_deployment(
        &mut self,
        release: Release,
    ) -> ResponseActFuture<Self, Result<Release, ()>> {
        log::info!(
            "staged deployment '{}' available, proceeding to finalize it",
            release.version
        );

        let msg = rpm_ostree::FinalizeDeployment { release };
        let upgrade = self
            .rpm_ostree_actor
            .send(msg)
            .unwrap_or_else(|e| Err(e.into()))
            .map_err(|e| log::error!("failed to finalize deployment: {}", e))
            .into_actor(self);

        Box::pin(upgrade)
    }

    /// Attempt to register as the update driver for rpm-ostree.
    fn register_as_driver(&mut self) -> ResponseActFuture<UpdateAgent, Result<(), ()>> {
        log::info!("registering as the update driver for rpm-ostree");

        let msg = rpm_ostree::RegisterAsDriver {};
        let result = self
            .rpm_ostree_actor
            .send(msg)
            .unwrap_or_else(|e| Err(e.into()))
            .map_err(|e| log::error!("failed to register as driver: {}", e))
            .into_actor(self);

        Box::pin(result)
    }

    /// Do nothing, without errors.
    fn nop(&mut self) -> ResponseActFuture<Self, Result<(), ()>> {
        let nop = actix::fut::ok(());
        Box::pin(nop)
    }
}

/// Helper function to send notification to the service manager about service status changes.
/// Log errors if unsuccessful.
fn update_unit_status(status: &str) {
    match notify(false, &[NotifyState::Status(status.to_string())]) {
        Err(e) => log::error!(
            "failed to notify service manager about service status change: {}",
            e
        ),
        Ok(sent) => {
            if !sent {
                log::error!(
                    "update_unit_status: status notifications not supported for this service"
                );
            }
        }
    }
}

/// Helper function to tell the service manager that Zincati start up is finished and
/// configuration is loaded.
fn notify_ready() {
    match notify(false, &[NotifyState::Ready]) {
        Err(e) => log::error!(
            "failed to notify service manager that service is ready: {}",
            e
        ),
        Ok(sent) => {
            if !sent {
                log::error!("notify_ready: status notifications not supported for this service");
            }
        }
    }
}
