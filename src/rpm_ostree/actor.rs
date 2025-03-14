//! rpm-ostree client actor.

use super::cli_status::Status;
use super::{Payload, Release};
use actix::prelude::*;
use anyhow::{Context, Result};
use filetime::FileTime;
use log::trace;
use ostree_ext::container::OstreeImageReference;
use ostree_ext::oci_spec::distribution::Reference;
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
        let rebase_target = match &msg.release.payload {
            Payload::Pullspec(target_pullspec) => {
                // If there is staged deployment we use that to determine if we should rebase or deploy
                // Otherwise, fallback to booted.
                // This is because if we already staged a rebase, rebasing again won't work
                // as "Old and new refs are equal"
                // see https://github.com/coreos/szincati/pull/1273#issuecomment-2721531804
                let status = super::cli_status::invoke_cli_status(false)?;
                let local_deploy = match super::cli_status::get_staged_deployment(&status) {
                    Some(staged_deploy) => staged_deploy,
                    None => super::cli_status::booted_status(&status)?,
                };

                if let Some(booted_imgref) = local_deploy.get_container_image_reference() {
                    let booted_oci_ref: Reference = booted_imgref.imgref.name.parse()?;
                    let stream = local_deploy.get_fcos_update_stream()?;

                    // The cinncinati payload contains the container image pullspec, pinned to a digest.
                    // There are two cases where we want to rebase to a OSTree OCI refspec.
                    // 1 - The image we are booted on does not match a stream tag, e.g. the node was manually
                    //     rebased to a version tag or a pinned digest. Here deploy would work but would lead
                    //     to a weird UX:
                    //     rpm-ostree status would show the version tag in the origin after we moved on to
                    //     another version.
                    // 2 - The image name we are following has changed (new registry, new name)
                    //     In that case `deploy` won't work and we need to rebase to the new refspec.

                    // The oci reference we want to end up with
                    let tagged_rebase_ref = Reference::with_tag(
                        target_pullspec.registry().to_string(),
                        target_pullspec.repository().to_string(),
                        stream,
                    );

                    // if those don't match we need to rebase
                    if booted_oci_ref != tagged_rebase_ref {
                        // craft a new ostree imgref object with the tagged oci reference we'll use for
                        // the rebase command so rpm-ostree will verify the signature of the OSTree commit
                        // wrapped inside the container:
                        let rebase_target = OstreeImageReference {
                            sigverify: booted_imgref.sigverify,
                            imgref: ostree_ext::container::ImageReference {
                                transport: booted_imgref.imgref.transport,
                                name: tagged_rebase_ref.whole(),
                            },
                        };
                        Some(rebase_target)
                    } else {
                        None
                    }
                } else {
                    // This should never happen as requesting the OCI graph only happens after we detected the local deployment is OCI.
                    // But let's fail gracefuly just in case.
                    anyhow::bail!("Zincati does not support OCI updates if the current deployment is not already an OCI image reference.")
                }
            }
            Payload::Checksum(_) => None,
        };
        trace!("request to stage release: {:?}", &msg.release);
        let release =
            super::cli_deploy::deploy_locked(msg.release, msg.allow_downgrade, rebase_target);
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
