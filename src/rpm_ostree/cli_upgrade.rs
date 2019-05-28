//! Interface to `rpm-ostree upgrade --lock-finalization`.

use super::Release;
use failure::{bail, format_err, Fallible, ResultExt};

/// Upgrade and leave the new deployment locked.
pub fn locked_upgrade(release: Release) -> Fallible<Release> {
    let cmd = std::process::Command::new("rpm-ostree")
        .arg("deploy")
        .arg("--lock-finalization")
        .arg(release.reference_id())
        .output()
        .with_context(|e| format_err!("failed to run rpm-ostree: {}", e))?;

    if !cmd.status.success() {
        bail!(
            "rpm-ostree upgrade failed:\n{}",
            String::from_utf8_lossy(&cmd.stderr)
        );
    }

    Ok(release)
}
