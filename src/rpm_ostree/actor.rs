//! rpm-ostree client actor.

use super::cli_status::Status;
use super::{Payload, Release};
use actix::prelude::*;
use anyhow::{Context, Result};
use filetime::FileTime;
use log::trace;
use ostree_ext::container::OstreeImageReference;
use std::collections::BTreeSet;
use std::rc::Rc;

/// Cache of local deployments.
#[derive(Clone, Debug)]
pub struct StatusCache {
    pub status: Rc<Status>,
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
    type Result = Result<Release>;
}

impl Handler<StageDeployment> for RpmOstreeClient {
    type Result = Result<Release>;

    fn handle(&mut self, msg: StageDeployment, _ctx: &mut Self::Context) -> Self::Result {
        let booted = super::cli_status::invoke_cli_status(true)?;
        let local_deploy = super::cli_status::booted_status(&booted)?;

        let release = if let Payload::Pullspec(release_payload) = msg.release.payload {
            if let Some(local_imgref) = local_deploy.container_image_reference() {
                // re-use the custom origin info to the new deployment
                // while updating custom-url to match the new image digest
                // XXX: when moving to update graph V2 we want to remove this and
                // have custom-url pointing to the OCI artifact containing the graph
                // See https://github.com/coreos/fedora-coreos-tracker/issues/1872
                let custom_origin = if let Some(mut custom_origin) = local_deploy.custom_origin() {
                    custom_origin.url = release_payload.clone();
                    Some(custom_origin)
                } else {
                    log::warn!("Missing custom origin information for local OCI deployment.");
                    None
                };

                // Cinncinati payload contains the container image pullspec, but we need
                // to prepend the OSTree signature source so rpm-ostree will verify the signature of
                // the OSTree commit wrapped inside the container.

                // let's craft a propper ostree imgref object
                let rebase_target = OstreeImageReference {
                    sigverify: local_imgref.sigverify,
                    imgref: ostree_ext::container::ImageReference {
                        transport: ostree_ext::container::Transport::Registry,
                        name: release_payload,
                    },
                };

                // re-craft a release object with the pullspec
                let oci_release = Release {
                    version: msg.release.version.clone(),
                    payload: Payload::Pullspec(rebase_target.to_string()),
                    age_index: msg.release.age_index,
                };

                trace!("request to stage release: {:?}", oci_release);
                super::cli_deploy::deploy_locked(oci_release, msg.allow_downgrade, custom_origin)
            } else {
                // This should never happen as requesting the OCI graph only happens after we detected the local deployment is OCI.
                // But let's fail gracefuly just in case.
                anyhow::bail!("Zincati does not support OCI updates if the current deployment is not already an OCI image reference.")
            }
        } else {
            trace!("request to stage release: {:?}", msg.release);
            super::cli_deploy::deploy_locked(msg.release, msg.allow_downgrade, None)
        };
        trace!("rpm-ostree CLI returned: {:?}", release);
        release
    }
}

/// Request: finalize a staged deployment (by unlocking it and rebooting).
#[derive(Debug, Clone)]
pub struct FinalizeDeployment {
    /// Finalized release to finalize.
    pub release: Release,
}

impl Message for FinalizeDeployment {
    type Result = Result<Release>;
}

impl Handler<FinalizeDeployment> for RpmOstreeClient {
    type Result = Result<Release>;

    fn handle(&mut self, msg: FinalizeDeployment, _ctx: &mut Self::Context) -> Self::Result {
        trace!("request to finalize release: {:?}", msg.release);
        let release = super::cli_finalize::finalize_deployment(msg.release);
        trace!("rpm-ostree CLI returned: {:?}", release);
        release
    }
}

/// Request: query local deployments.
#[derive(Debug, Clone)]
pub struct QueryLocalDeployments {
    /// Whether to include staged (i.e. not finalized) deployments in query result.
    pub(crate) omit_staged: bool,
}

impl Message for QueryLocalDeployments {
    type Result = Result<BTreeSet<Release>>;
}

impl Handler<QueryLocalDeployments> for RpmOstreeClient {
    type Result = Result<BTreeSet<Release>>;

    fn handle(
        &mut self,
        query_msg: QueryLocalDeployments,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        trace!("request to list local deployments");
        let releases = super::cli_status::local_deployments(self, query_msg.omit_staged);
        trace!("rpm-ostree CLI returned: {:?}", releases);
        releases
    }
}

/// Request: query pending deployment and stream.
#[derive(Debug, Clone)]
pub struct QueryPendingDeploymentStream {}

impl Message for QueryPendingDeploymentStream {
    type Result = Result<Option<(Release, String)>>;
}

impl Handler<QueryPendingDeploymentStream> for RpmOstreeClient {
    type Result = Result<Option<(Release, String)>>;

    fn handle(
        &mut self,
        _msg: QueryPendingDeploymentStream,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        trace!("fetching details for staged deployment");

        let status = super::cli_status::invoke_cli_status(false)?;
        super::cli_status::parse_pending_deployment(&status)
            .context("failed to introspect pending deployment")
    }
}

/// Request: cleanup pending deployment.
#[derive(Debug, Clone)]
pub struct CleanupPendingDeployment {}

impl Message for CleanupPendingDeployment {
    type Result = Result<()>;
}

impl Handler<CleanupPendingDeployment> for RpmOstreeClient {
    type Result = Result<()>;

    fn handle(&mut self, _msg: CleanupPendingDeployment, _ctx: &mut Self::Context) -> Self::Result {
        trace!("request to cleanup pending deployment");
        super::cli_deploy::invoke_cli_cleanup()?;
        Ok(())
    }
}

/// Request: Register as the update driver for rpm-ostree.
#[derive(Debug, Clone)]
pub struct RegisterAsDriver {}

impl Message for RegisterAsDriver {
    type Result = ();
}

impl Handler<RegisterAsDriver> for RpmOstreeClient {
    type Result = ();

    fn handle(&mut self, _msg: RegisterAsDriver, _ctx: &mut Self::Context) -> Self::Result {
        trace!("request to register as rpm-ostree update driver");
        super::cli_deploy::deploy_register_driver()
    }
}
