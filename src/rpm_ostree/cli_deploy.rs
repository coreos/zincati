//! Interface to `rpm-ostree deploy --lock-finalization`.

use super::Release;
use failure::{bail, format_err, Fallible, ResultExt};
use prometheus::IntCounter;

lazy_static::lazy_static! {
    static ref DEPLOY_ATTEMPTS: IntCounter = register_int_counter!(opts!(
        "zincati_rpm_ostree_deploy_attempts_total",
        "Total number of 'rpm-ostree deploy' attempts."
    )).unwrap();
    static ref DEPLOY_FAILURES: IntCounter = register_int_counter!(opts!(
        "zincati_rpm_ostree_deploy_failures_total",
        "Total number of 'rpm-ostree deploy' failures."
    )).unwrap();
}

/// Deploy an upgrade (by checksum) and leave the new deployment locked.
pub fn deploy_locked(release: Release) -> Fallible<Release> {
    DEPLOY_ATTEMPTS.inc();
    let cmd = std::process::Command::new("rpm-ostree")
        .arg("deploy")
        .arg("--lock-finalization")
        .arg(format!("revision={}", release.checksum))
        .output()
        .with_context(|e| format_err!("failed to run rpm-ostree: {}", e))?;

    if !cmd.status.success() {
        DEPLOY_FAILURES.inc();
        bail!(
            "rpm-ostree deploy failed:\n{}",
            String::from_utf8_lossy(&cmd.stderr)
        );
    }

    Ok(release)
}
