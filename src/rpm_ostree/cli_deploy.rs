//! Interface to `rpm-ostree deploy --lock-finalization` and
//! `rpm-ostree deploy --register-driver`.

use crate::rpm_ostree::{Payload, Release};
use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::Lazy;
use ostree_ext::container::OstreeImageReference;
use prometheus::IntCounter;
use std::time::Duration;

const DRIVER_NAME: &str = "Zincati";

static DEPLOY_ATTEMPTS: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(opts!(
        "zincati_rpm_ostree_deploy_attempts_total",
        "Total number of 'rpm-ostree deploy' attempts."
    ))
    .unwrap()
});
static DEPLOY_FAILURES: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(opts!(
        "zincati_rpm_ostree_deploy_failures_total",
        "Total number of 'rpm-ostree deploy' failures."
    ))
    .unwrap()
});
static REGISTER_DRIVER_FAILURES: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(opts!(
        "zincati_rpm_ostree_register_driver_failures_total",
        "Total number of failures to register as driver for rpm-ostree."
    ))
    .unwrap()
});

/// Deploy an upgrade (by checksum) and leave the new deployment locked.
pub fn deploy_locked(
    release: Release,
    allow_downgrade: bool,
    rebase: Option<OstreeImageReference>,
) -> Result<Release> {
    DEPLOY_ATTEMPTS.inc();

    let result = invoke_cli_deploy(release, allow_downgrade, rebase);
    if result.is_err() {
        DEPLOY_FAILURES.inc();
    }

    result
}

/// Register as the update driver.
/// Keep attempting to register as driver for rpm-ostree, with exponential backoff
/// capped at 256 seconds.
pub fn deploy_register_driver() {
    let mut register_attempt = invoke_cli_register();
    let mut retry_secs = Duration::from_secs(1);
    while let Err(attempt) = register_attempt {
        REGISTER_DRIVER_FAILURES.inc();
        log::error!("{}\nretrying in {:?}", attempt, retry_secs,);
        // Use `std::thread::sleep` because the rpm-ostree actor is spawned in a SyncArbiter.
        std::thread::sleep(retry_secs);
        register_attempt = invoke_cli_register();
        if retry_secs < Duration::from_secs(256) {
            retry_secs *= 2;
        }
    }
}

/// CLI executor for registering driver.
fn invoke_cli_register() -> Result<()> {
    // `fail_point`s cause registration to fail on first 3 tries when unit testing.
    fail_point!(
        "register_driver_err",
        REGISTER_DRIVER_FAILURES.get() < 2,
        |_| bail!("register_driver_err")
    );
    fail_point!(
        "register_driver_ok",
        REGISTER_DRIVER_FAILURES.get() >= 3,
        |_| Ok(())
    );

    let mut cmd = std::process::Command::new("rpm-ostree");
    cmd.arg("deploy")
        .arg("")
        .arg(format!("--register-driver={}", DRIVER_NAME))
        .env("RPMOSTREE_CLIENT_ID", "zincati");

    let out = cmd.output().context("failed to run 'rpm-ostree' binary")?;

    if !out.status.success() {
        bail!(
            "rpm-ostree deploy --register-driver failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(())
}

/// CLI executor for deploying upgrades.
fn invoke_cli_deploy(
    release: Release,
    allow_downgrade: bool,
    rebase: Option<OstreeImageReference>,
) -> Result<Release> {
    fail_point!("deploy_locked_err", |_| bail!("deploy_locked_err"));
    fail_point!("deploy_locked_ok", |_| Ok(release.clone()));

    let mut cmd = std::process::Command::new("rpm-ostree");
    match &release.payload {
        Payload::Pullspec(reference) => {
            if let Some(rebase_target) = rebase {
                cmd.arg("rebase").arg("--lock-finalization");
                let digest = reference
                    .digest()
                    .ok_or_else(|| anyhow!("Missing digest in Cincinnati payload"))?;
                cmd.arg(rebase_target.to_string()).arg(digest);
            } else {
                cmd.arg("deploy")
                    .arg("--lock-finalization")
                    .arg(reference.digest().unwrap());
            }
        }
        Payload::Checksum(checksum) => {
            cmd.arg("deploy")
                .arg("--lock-finalization")
                .arg("--skip-branch-check")
                .arg(format!("revision={}", checksum));
        }
    }
    cmd.env("RPMOSTREE_CLIENT_ID", "zincati");
    if !allow_downgrade {
        cmd.arg("--disallow-downgrade");
    }
    log::trace!(
        "Requesting rpm ostree deploy with arguments: {:?}",
        cmd.get_args()
    );

    let out = cmd.output().context("failed to run 'rpm-ostree' binary")?;

    if !out.status.success() {
        bail!(
            "rpm-ostree deploy failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(release)
}

/// CLI executor for cleaning up the pending deployment.
pub fn invoke_cli_cleanup() -> Result<()> {
    let mut cmd = std::process::Command::new("rpm-ostree");
    cmd.arg("cleanup").arg("-p");
    let out = cmd.output().context("failed to run 'rpm-ostree' binary")?;
    if !out.status.success() {
        bail!(
            "rpm-ostree cleanup failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    };
    Ok(())
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
            payload: Payload::Checksum("bar".to_string()),
            age_index: None,
        };
        let result = deploy_locked(release, false, None);
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
            payload: Payload::Checksum("bar".to_string()),
            age_index: None,
        };
        let result = deploy_locked(release.clone(), false, None).unwrap();
        assert_eq!(result, release);
        assert!(DEPLOY_ATTEMPTS.get() >= 1);
    }

    #[cfg(feature = "failpoints")]
    #[test]
    fn register_driver_err() {
        use std::time::SystemTime;

        let _guard = fail::FailScenario::setup();
        fail::cfg("register_driver_err", "return").unwrap();
        fail::cfg("register_driver_ok", "return").unwrap();

        let now = SystemTime::now();
        // expect to take 1 + 2 + 4 = 7 seconds
        // to register as driver due to `fail_point`s
        deploy_register_driver();
        let elapsed = now.elapsed().unwrap().as_secs();
        // `fail_point`s are set to succeed on 4th try
        assert!(REGISTER_DRIVER_FAILURES.get() == 3);
        assert!(elapsed >= 7);
    }
}
