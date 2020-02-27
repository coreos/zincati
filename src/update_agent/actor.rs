//! Update agent actor.

use super::{UpdateAgent, UpdateAgentState};
use crate::rpm_ostree::{self, Release};
use actix::prelude::*;
use failure::Error;
use futures::prelude::*;
use log::trace;
use prometheus::IntGauge;
use std::collections::BTreeSet;
use std::time::Duration;

lazy_static::lazy_static! {
    static ref ALLOW_DOWNGRADE: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_updates_allow_downgrade",
        "Whether downgrades via auto-updates logic are allowed."
    )).unwrap();
    static ref LAST_REFRESH: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_last_refresh_timestamp",
        "UTC timestamp of update-agent last refresh tick."
    )).unwrap();
    static ref UPDATES_ENABLED: IntGauge = register_int_gauge!(opts!(
        "zincati_update_agent_updates_enabled",
        "Whether auto-updates logic is enabled."
    )).unwrap();
}

impl Actor for UpdateAgent {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        trace!("update agent started");

        // TODO(lucab): consider adding more metrics here (e.g. steady interval).
        UPDATES_ENABLED.set(i64::from(self.enabled));

        if self.allow_downgrade {
            ALLOW_DOWNGRADE.set(1);
            log::warn!("client configuration allows (possibly vulnerable) downgrades via auto-updates logic");
        }

        // Kick-start the state machine.
        Self::tick_now(ctx);
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
            UpdateAgentState::StartState => self.initialize(),
            UpdateAgentState::Initialized => self.try_steady(),
            UpdateAgentState::Steady => self.try_check_updates(),
            UpdateAgentState::UpdateAvailable(release) => {
                let update = release.clone();
                self.try_stage_update(update)
            }
            UpdateAgentState::UpdateStaged(release) => {
                let update = release.clone();
                self.try_finalize_update(update)
            }
            UpdateAgentState::UpdateFinalized(release) => {
                let update = release.clone();
                self.end(update)
            }
            UpdateAgentState::EndState => self.nop(),
        };

        let update_machine = state_action.then(move |_r, actor, ctx| {
            if prev_state != actor.state {
                let update_timestamp = chrono::Utc::now();
                actor.state_changed = update_timestamp;
                Self::tick_now(ctx);
            } else {
                let interval = actor.refresh_delay();
                let pause = Self::add_jitter(interval);
                log::trace!(
                    "scheduling next agent refresh in {} seconds",
                    pause.as_secs()
                );
                Self::tick_later(ctx, pause);
            }
            actix::fut::ready(())
        });

        // Process state machine refresh ticks sequentially.
        ctx.wait(update_machine);

        Box::new(actix::fut::ok(()))
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
    fn refresh_delay(&self) -> Duration {
        let default_delay = Duration::from_secs(super::DEFAULT_REFRESH_PERIOD_SECS);

        match self.state {
            UpdateAgentState::Steady => self.steady_interval,
            _ => default_delay,
        }
    }

    /// Add a small, random amount (0% to 10%) of jitter to a given period.
    ///
    /// This random jitter is useful to prevent clients from converging to
    /// the same phase-locked loop.
    fn add_jitter(period: std::time::Duration) -> std::time::Duration {
        use rand::Rng;

        let secs = period.as_secs();
        let rand: u8 = rand::thread_rng().gen_range(0, 11);
        let jitter = u64::max(secs / 100, 1).saturating_mul(u64::from(rand));
        std::time::Duration::from_secs(secs.saturating_add(jitter))
    }

    /// Initialize the update agent.
    fn initialize(&mut self) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("update agent in start state");

        let initialization = self.nop().map(|_r, actor, _ctx| {
            if actor.enabled {
                log::info!("initialization complete, auto-updates logic enabled");
                actor.state.initialized();
            } else {
                log::warn!("initialization complete, auto-updates logic disabled by configuration");
                actor.state.end();
            };
            Ok(())
        });

        Box::new(initialization)
    }

    /// Try to reach steady state.
    fn try_steady(&mut self) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("trying to report steady state");

        let report_steady = self.strategy.report_steady(&self.identity);
        let state_change =
            actix::fut::wrap_future::<_, Self>(report_steady).map(|is_steady, actor, _ctx| {
                if is_steady {
                    log::debug!("reached steady state, periodically polling for updates");
                    actor.state.steady();
                }
                Ok(())
            });

        Box::new(state_change)
    }

    /// Try to check for updates.
    fn try_check_updates(&mut self) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("trying to check for updates");

        let can_check = self.strategy.can_check_and_fetch(&self.identity);
        let state_change = actix::fut::wrap_future::<_, Self>(can_check)
            .then(|can_check, actor, _ctx| actor.local_deployments(can_check))
            .then(|res, actor, _ctx| {
                let allow_downgrade = actor.allow_downgrade;
                let release = match res {
                    Ok((can_check, depls)) => actor.cincinnati.fetch_update_hint(
                        &actor.identity,
                        depls,
                        can_check,
                        allow_downgrade,
                    ),
                    _ => Box::pin(futures::future::ready(None)),
                };
                release.into_actor(actor)
            })
            .map(|res, actor, _ctx| {
                actor.state.update_available(res);
                Ok(())
            });

        Box::new(state_change)
    }

    /// Try to stage an update.
    fn try_stage_update(&mut self, release: Release) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("trying to stage an update");

        let can_fetch = self.strategy.can_check_and_fetch(&self.identity);
        let state_change = actix::fut::wrap_future::<_, Self>(can_fetch)
            .then(|can_fetch, actor, _ctx| actor.locked_upgrade(can_fetch, release))
            .map(|res, actor, _ctx| res.map(|release| actor.state.update_staged(release)));

        Box::new(state_change)
    }

    /// Try to finalize an update.
    fn try_finalize_update(&mut self, release: Release) -> ResponseActFuture<Self, Result<(), ()>> {
        trace!("trying to finalize an update");

        let can_finalize = self.strategy.can_finalize(&self.identity);
        let state_change = actix::fut::wrap_future::<_, Self>(can_finalize)
            .then(|can_finalize, actor, _ctx| actor.finalize_deployment(can_finalize, release))
            .map(|res, actor, _ctx| res.map(|release| actor.state.update_finalized(release)));

        Box::new(state_change)
    }

    /// Actor job is done.
    fn end(&mut self, release: Release) -> ResponseActFuture<Self, Result<(), ()>> {
        log::info!("update applied, waiting for reboot: {}", release.version);
        let state_change = self.nop().map(|_r, actor, _ctx| {
            actor.state.end();
            Ok(())
        });

        Box::new(state_change)
    }

    /// Fetch and stage an update, in finalization-locked mode.
    fn locked_upgrade(
        &mut self,
        can_fetch: bool,
        release: Release,
    ) -> ResponseActFuture<Self, Result<Release, ()>> {
        if !can_fetch {
            return Box::new(actix::fut::err(()));
        }

        log::info!(
            "new release '{}' selected, proceeding to stage it",
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

        Box::new(upgrade)
    }

    /// List local deployments.
    fn local_deployments(
        &mut self,
        can_fetch: bool,
    ) -> ResponseActFuture<Self, Result<(bool, BTreeSet<Release>), ()>> {
        if !can_fetch {
            return Box::new(actix::fut::ok((can_fetch, BTreeSet::new())));
        }

        let msg = rpm_ostree::QueryLocalDeployments {};
        let depls = self
            .rpm_ostree_actor
            .send(msg)
            .unwrap_or_else(|e| Err(e.into()))
            .map_err(|e| log::error!("failed to query local deployments: {}", e))
            .map_ok(move |depls| {
                log::trace!("found {} local deployments", depls.len());
                (can_fetch, depls)
            })
            .into_actor(self);

        Box::new(depls)
    }

    /// Finalize a deployment (unlock and reboot).
    fn finalize_deployment(
        &mut self,
        can_finalize: bool,
        release: Release,
    ) -> ResponseActFuture<Self, Result<Release, ()>> {
        if !can_finalize {
            return Box::new(actix::fut::err(()));
        }

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

        Box::new(upgrade)
    }

    /// Do nothing, without errors.
    fn nop(&mut self) -> ResponseActFuture<Self, Result<(), ()>> {
        let nop = actix::fut::ok(());
        Box::new(nop)
    }
}
