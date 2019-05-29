//! Interface to `rpm-ostree finalize-deployment`.

use super::Release;
use failure::{bail, format_err, Fallible, ResultExt};

/// Unlock and finalize the new deployment.
pub fn finalize_deployment(release: Release) -> Fallible<Release> {
    let cmd = std::process::Command::new("rpm-ostree")
        .arg("finalize-deployment")
        .arg(&release.checksum)
        .output()
        .with_context(|e| format_err!("failed to run rpm-ostree: {}", e))?;

    if !cmd.status.success() {
        bail!(
            "rpm-ostree finalize-deployment failed:\n{}",
            String::from_utf8_lossy(&cmd.stderr)
        );
    }

    Ok(release)
}
