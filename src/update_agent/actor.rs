//! Update agent actor.

use super::{UpdateAgent, UpdateAgentState};
use actix::prelude::*;
use failure::Error;
use log::trace;

impl Actor for UpdateAgent {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        trace!("update agent started");

        // Kick-start the state machine.
        Self::tick_now(ctx);
    }
}

pub(crate) struct RefreshTick {}

impl Message for RefreshTick {
    type Result = Result<(), Error>;
}

impl Handler<RefreshTick> for UpdateAgent {
    type Result = ResponseActFuture<Self, (), Error>;

    fn handle(&mut self, _msg: RefreshTick, ctx: &mut Self::Context) -> Self::Result {
        trace!("update agent tick, current state: {:?}", self.state);
        let prev_state = self.state.clone();

        let state_action = match &self.state {
            UpdateAgentState::StartState => self.initialize(),
            UpdateAgentState::Initialized => self.try_steady(),
            UpdateAgentState::Steady => self.try_check_updates(),
            UpdateAgentState::UpdateAvailable(_release) => self.todo(),
            UpdateAgentState::_EndState => self.nop(),
        };

        let update_machine = state_action.then(move |_r, actor, ctx| {
            if prev_state != actor.state {
                let now = chrono::Utc::now();
                actor.state_changed = now;
                Self::tick_now(ctx);
            } else {
                Self::tick_later(ctx, actor.refresh_period);
            }
            actix::fut::ok(())
        });

        // Process state machine refresh ticks sequentially.
        ctx.wait(update_machine);

        Box::new(actix::fut::ok(()))
    }
}

impl UpdateAgent {
    /// Schedule an immediate refresh the state machine.
    pub fn tick_now(ctx: &mut Context<Self>) {
        ctx.notify(RefreshTick {})
    }

    /// Schedule a delayed refresh of the state machine.
    pub fn tick_later(ctx: &mut Context<Self>, after: std::time::Duration) -> actix::SpawnHandle {
        ctx.notify_later(RefreshTick {}, after)
    }

    /// Initialize the update agent.
    fn initialize(&mut self) -> ResponseActFuture<Self, (), ()> {
        trace!("update agent in start state");

        let initialization = self.nop().map(|_r, actor, _ctx| {
            actor.state.initialized();
        });

        Box::new(initialization)
    }

    /// Try to reach steady state.
    fn try_steady(&mut self) -> ResponseActFuture<Self, (), ()> {
        trace!("trying to report steady state");

        let report_steady = self.strategy.report_steady(&self.identity);
        let state_change =
            actix::fut::wrap_future::<_, Self>(report_steady).map(|is_steady, actor, _ctx| {
                actor.state.steady(is_steady);
            });

        Box::new(state_change)
    }

    /// Try to check for and stage updates.
    fn try_check_updates(&mut self) -> ResponseActFuture<Self, (), ()> {
        trace!("trying to check for updates");

        let can_check = self.strategy.can_check_and_fetch(&self.identity);
        let state_change = actix::fut::wrap_future::<_, Self>(can_check)
            .and_then(|can_check, actor, _ctx| {
                actor
                    .cincinnati
                    .fetch_update_hint(&actor.identity, can_check)
                    .into_actor(actor)
            })
            .map(|update, actor, _ctx| actor.state.update_available(update));

        Box::new(state_change)
    }

    /// Do nothing, without errors.
    fn nop(&mut self) -> ResponseActFuture<Self, (), ()> {
        let nop = actix::fut::ok(());
        Box::new(nop)
    }

    /// Pending implementation.
    fn todo(&mut self) -> ResponseActFuture<Self, (), ()> {
        log::error!("pending implementation");
        self.nop()
    }
}
