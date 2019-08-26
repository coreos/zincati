//! rpm-ostree client actor.

use super::Release;
use actix::prelude::*;
use failure::Fallible;
use log::trace;
use std::collections::BTreeSet;

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
    /// Whether to allow downgrades.
    pub allow_downgrade: bool,
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
        super::cli_deploy::deploy_locked(msg.release, msg.allow_downgrade)
    }
}

/// Request: finalize a staged deployment (by unlocking it and rebooting).
#[derive(Debug, Clone)]
pub struct FinalizeDeployment {
    /// Finalized release to finalize.
    pub release: Release,
}

impl Message for FinalizeDeployment {
    type Result = Fallible<Release>;
}

impl Handler<FinalizeDeployment> for RpmOstreeClient {
    type Result = Fallible<Release>;

    fn handle(&mut self, msg: FinalizeDeployment, _ctx: &mut Self::Context) -> Self::Result {
        trace!("request to finalize release: {:?}", msg.release);
        super::cli_finalize::finalize_deployment(msg.release)
    }
}

/// Request: query local deployments.
#[derive(Debug, Clone)]
pub struct QueryLocalDeployments {}

impl Message for QueryLocalDeployments {
    type Result = Fallible<BTreeSet<Release>>;
}

impl Handler<QueryLocalDeployments> for RpmOstreeClient {
    type Result = Fallible<BTreeSet<Release>>;

    fn handle(&mut self, _msg: QueryLocalDeployments, _ctx: &mut Self::Context) -> Self::Result {
        trace!("request to list local deployments");
        super::cli_status::local_deployments()
    }
}
