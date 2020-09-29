//! Interface to `rpm-ostree deploy --lock-finalization`.

use super::Release;
use failure::{bail, Fallible, ResultExt};
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
pub fn deploy_locked(release: Release, allow_downgrade: bool) -> Fallible<Release> {
    DEPLOY_ATTEMPTS.inc();

    let result = invoke_cli(release, allow_downgrade);
    if result.is_err() {
        DEPLOY_FAILURES.inc();
    }

    result
}

/// CLI executor.
fn invoke_cli(release: Release, allow_downgrade: bool) -> Fallible<Release> {
    fail_point!("deploy_locked_err", |_| bail!("deploy_locked_err"));
    fail_point!("deploy_locked_ok", |_| Ok(release.clone()));

    let mut cmd = std::process::Command::new("rpm-ostree");
    cmd.arg("deploy")
        .arg("--lock-finalization")
        .arg(format!("revision={}", release.checksum))
        .env("RPMOSTREE_CLIENT_ID", "zincati");
    if !allow_downgrade {
        cmd.arg("--disallow-downgrade");
    }

    let out = cmd
        .output()
        .with_context(|_| "failed to run 'rpm-ostree' binary")?;

    if !out.status.success() {
        bail!(
            "rpm-ostree deploy failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(release)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(feature = "failpoints")]
    #[test]
    fn deploy_locked_err() {
        let _guard = fail::FailScenario::setup();
        fail::cfg("deploy_locked_err", "return").unwrap();

        let release = Release {
            version: "foo".to_string(),
            checksum: "bar".to_string(),
            age_index: None,
        };
        let result = deploy_locked(release, true);
        assert!(result.is_err());
        assert!(DEPLOY_ATTEMPTS.get() >= 1);
        assert!(DEPLOY_FAILURES.get() >= 1);
    }

    #[cfg(feature = "failpoints")]
    #[test]
    fn deploy_locked_ok() {
        let _guard = fail::FailScenario::setup();
        fail::cfg("deploy_locked_ok", "return").unwrap();

        let release = Release {
            version: "foo".to_string(),
            checksum: "bar".to_string(),
            age_index: None,
        };
        let result = deploy_locked(release.clone(), true).unwrap();
        assert_eq!(result, release);
        assert!(DEPLOY_ATTEMPTS.get() >= 1);
    }
}
