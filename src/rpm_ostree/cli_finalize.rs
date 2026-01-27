//! Interface to `rpm-ostree finalize-deployment`.

use super::Release;
use anyhow::{anyhow, bail, Context, Result};
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

    match &release.payload {
        super::Payload::Pullspec(reference) => {
            let digest = reference
                .digest()
                .ok_or_else(|| anyhow!("Missing digest in Cincinnati payload"))?;
            cmd.arg(digest);
        }
    };

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
