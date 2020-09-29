//! Interface to `rpm-ostree finalize-deployment`.

use super::Release;
use failure::{bail, Fallible, ResultExt};
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
pub fn finalize_deployment(release: Release) -> Fallible<Release> {
    FINALIZE_ATTEMPTS.inc();
    let cmd = std::process::Command::new("rpm-ostree")
        .arg("finalize-deployment")
        .arg(&release.checksum)
        .env("RPMOSTREE_CLIENT_ID", "zincati")
        .output()
        .with_context(|_| "failed to run 'rpm-ostree' binary")?;

    if !cmd.status.success() {
        FINALIZE_FAILURES.inc();
        bail!(
            "rpm-ostree finalize-deployment failed:\n{}",
            String::from_utf8_lossy(&cmd.stderr)
        );
    }

    Ok(release)
}
