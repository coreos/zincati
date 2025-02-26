//! Interface to `rpm-ostree finalize-deployment`.

use super::Release;
use anyhow::{bail, Context, Result};
use prometheus::IntCounter;

lazy_static::lazy_static! {
    static ref FINALIZE_ATTEMPTS: IntCounter = register_int_counter!(opts!(
        "zincati_rpm_ostree_finalize_attempts_total",
        "Total number of 'rpm-ostree finalize-deployment' attempts."
    )).unwrap();
    static ref FINALIZE_FAILURES: IntCounter = register_int_counter!(opts!(
        "zincati_rpm_ostree_finalize_failures_total",
        "Total number of 'rpm-ostree finalize-deployment' failures."
    )).unwrap();
}

/// Unlock and finalize the new deployment.
pub fn finalize_deployment(release: Release) -> Result<Release> {
    FINALIZE_ATTEMPTS.inc();
    let mut cmd = std::process::Command::new("rpm-ostree");
    cmd.env("RPMOSTREE_CLIENT_ID", "zincati")
        .arg("finalize-deployment");

    // XXX for OCI image, we don't know the checksum until we deployed it.
    // Currently, rpm-ostree do not return the resulting ostree commit
    // when rebasing to an OCI image. We query the deployments to get
    // the commit for the staged deployment.
    match &release.payload {
        super::Payload::Pullspec(release_imgref) => {
            let status = super::cli_status::invoke_cli_status(false)?;
            let staged = super::cli_status::get_staged_deployment(&status);
            if let Some(staged_depl) = staged {
                let staged_imgref = staged_depl
                    .container_image_reference()
                    .map(|i| i.to_string());
                if staged_imgref.as_ref() == Some(release_imgref) {
                    cmd.arg(staged_depl.ostree_checksum())
                } else {
                    bail!("The staged deployment does not match the update reference. Won't finalize.");
                }
            } else {
                bail!("No staged deployment to finalize.");
            };
        }
        super::Payload::Checksum(checksum) => {
            cmd.arg(checksum);
        }
    }

    let cmd_result = cmd.output().context("failed to run 'rpm-ostree' binary")?;
    if !cmd_result.status.success() {
        FINALIZE_FAILURES.inc();
        bail!(
            "rpm-ostree finalize-deployment failed:\n{}",
            String::from_utf8_lossy(&cmd_result.stderr)
        );
    }

    Ok(release)
}
