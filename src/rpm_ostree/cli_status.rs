//! Interface to `rpm-ostree status --json`.

#![allow(unused)]

use super::Release;
use failure::{bail, ensure, format_err, Fallible, ResultExt};
use serde::Deserialize;

/// JSON output from `rpm-ostree status --json`
#[derive(Debug, Deserialize)]
pub struct StatusJSON {
    deployments: Vec<DeploymentJSON>,
}

/// Partial deployment object (only fields relevant to zincati).
#[derive(Debug, Deserialize)]
pub struct DeploymentJSON {
    booted: bool,
    #[serde(rename = "base-checksum")]
    base_checksum: Option<String>,
    checksum: String,
    version: String,
}

impl DeploymentJSON {
    /// Convert into `Release`.
    pub fn into_release(self) -> Release {
        Release {
            checksum: self.base_revision(),
            version: self.version,
        }
    }

    /// Return the deployment base revision.
    pub fn base_revision(&self) -> String {
        self.base_checksum
            .clone()
            .unwrap_or_else(|| self.checksum.clone())
    }
}

/// Find the booted deployment.
pub fn booted() -> Fallible<Release> {
    let cmd = std::process::Command::new("rpm-ostree")
        .arg("status")
        .arg("--json")
        .arg("--booted")
        .output()
        .with_context(|e| format_err!("failed to run rpm-ostree: {}", e))?;

    if !cmd.status.success() {
        bail!(
            "rpm-ostree status failed:\n{}",
            String::from_utf8_lossy(&cmd.stderr)
        );
    }
    let status: StatusJSON = serde_json::from_slice(&cmd.stdout)?;

    let booted = status
        .deployments
        .into_iter()
        .find(|d| d.booted)
        .ok_or_else(|| format_err!("no booted deployment found"))?;

    ensure!(!booted.base_revision().is_empty(), "empty base revision");
    ensure!(!booted.version.is_empty(), "empty version");
    Ok(booted.into_release())
}
