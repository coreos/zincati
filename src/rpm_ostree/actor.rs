//! rpm-ostree client actor.

use super::cli_status::StatusJSON;
use super::Release;
use actix::prelude::*;
use failure::Fallible;
use filetime::FileTime;
use log::trace;
use std::collections::BTreeSet;

/// Cache of local deployments.
#[derive(Clone, Debug)]
pub struct StatusCache {
    pub status: StatusJSON,
    pub mtime: FileTime,
}

/// Client actor for rpm-ostree.
#[derive(Debug, Default, Clone)]
pub struct RpmOstreeClient {
    // NB: This is OK for now because `rpm-ostree` actor is curently spawned on a single thread,
    // but if we move to a larger threadpool, each actor thread will have its own cache.
    pub status_cache: Option<StatusCache>,
}

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
pub struct QueryLocalDeployments {
    /// Whether to include staged (i.e. not finalized) deployments in query result.
    pub(crate) omit_staged: bool,
}

impl Message for QueryLocalDeployments {
    type Result = Fallible<BTreeSet<Release>>;
}

impl Handler<QueryLocalDeployments> for RpmOstreeClient {
    type Result = Fallible<BTreeSet<Release>>;

    fn handle(
        &mut self,
        query_msg: QueryLocalDeployments,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        trace!("request to list local deployments");
        super::cli_status::local_deployments(self, query_msg.omit_staged)
    }
}

/// Request: Register as the update driver for rpm-ostree.
#[derive(Debug, Clone)]
pub struct RegisterAsDriver {}

impl Message for RegisterAsDriver {
    type Result = Fallible<()>;
}

impl Handler<RegisterAsDriver> for RpmOstreeClient {
    type Result = Fallible<()>;

    fn handle(&mut self, _msg: RegisterAsDriver, _ctx: &mut Self::Context) -> Self::Result {
        trace!("request to register as rpm-ostree update driver");
        super::cli_deploy::deploy_register_driver()
    }
}
