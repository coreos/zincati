//! rpm-ostree client actor.

use super::Release;
use actix::prelude::*;
use failure::Fallible;
use log::trace;

/// Client actor for rpm-ostree.
#[derive(Debug, Default, Clone)]
pub struct RpmOstreeClient {}

impl Actor for RpmOstreeClient {
    type Context = SyncContext<Self>;
}

impl RpmOstreeClient {
    /// Start the threadpool for rpm-ostree blocking clients.
    pub fn start(threads: usize) -> Addr<Self> {
        SyncArbiter::start(threads, RpmOstreeClient::default)
    }
}

/// Request: stage a deployment (in finalization-locked mode).
#[derive(Debug, Clone)]
pub struct StageDeployment {
    /// Release to be staged.
    pub release: Release,
}

impl Message for StageDeployment {
    type Result = Fallible<Release>;
}

impl Handler<StageDeployment> for RpmOstreeClient {
    type Result = Fallible<Release>;

    fn handle(&mut self, msg: StageDeployment, _ctx: &mut Self::Context) -> Self::Result {
        trace!("request to stage release: {:?}", msg.release);
        super::cli_upgrade::locked_upgrade(msg.release)
    }
}
