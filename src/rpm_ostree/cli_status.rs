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
    #[serde(rename = "base-commit-meta")]
    base_metadata: BaseCommitMetaJSON,
    checksum: String,
    version: String,
}

/// Metadata from base commit (only fields relevant to zincati).
#[derive(Debug, Deserialize)]
struct BaseCommitMetaJSON {
    #[serde(rename = "coreos-assembler.basearch")]
    basearch: String,
    #[serde(rename = "fedora-coreos.stream")]
    stream: String,
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

/// Return base architecture for booted deployment.
pub fn basearch() -> Fallible<String> {
    let status = status_json(true)?;
    let json = booted_json(status)?;
    Ok(json.base_metadata.basearch)
}

/// Find the booted deployment.
pub fn booted() -> Fallible<Release> {
    let status = status_json(true)?;
    let json = booted_json(status)?;
    Ok(json.into_release())
}

/// Return updates stream for booted deployment.
pub fn updates_stream() -> Fallible<String> {
    let status = status_json(true)?;
    let json = booted_json(status)?;
    ensure!(!json.base_metadata.stream.is_empty(), "empty stream value");
    Ok(json.base_metadata.stream)
}

/// Return JSON object for booted deployment.
fn booted_json(status: StatusJSON) -> Fallible<DeploymentJSON> {
    let booted = status
        .deployments
        .into_iter()
        .find(|d| d.booted)
        .ok_or_else(|| format_err!("no booted deployment found"))?;

    ensure!(!booted.base_revision().is_empty(), "empty base revision");
    ensure!(!booted.version.is_empty(), "empty version");
    ensure!(!booted.base_metadata.basearch.is_empty(), "empty basearch");
    Ok(booted)
}

/// Introspect deployments (rpm-ostree status).
fn status_json(booted_only: bool) -> Fallible<StatusJSON> {
    let mut cmd = std::process::Command::new("rpm-ostree");
    cmd.arg("status");

    // Try to request the minimum scope we need.
    if booted_only {
        cmd.arg("--booted");
    }

    let cmdrun = cmd
        .arg("--json")
        .output()
        .with_context(|e| format_err!("failed to run rpm-ostree: {}", e))?;

    if !cmdrun.status.success() {
        bail!(
            "rpm-ostree status failed:\n{}",
            String::from_utf8_lossy(&cmdrun.stderr)
        );
    }
    let status: StatusJSON = serde_json::from_slice(&cmdrun.stdout)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_status() -> Fallible<StatusJSON> {
        let fp = std::fs::File::open("tests/fixtures/rpm-ostree-status.json").unwrap();
        let mut bufrd = std::io::BufReader::new(fp);
        let status: StatusJSON = serde_json::from_reader(bufrd)?;
        Ok(status)
    }

    #[test]
    fn mock_booted_basearch() {
        let status = mock_status().unwrap();
        let booted = booted_json(status).unwrap();
        assert_eq!(booted.base_metadata.basearch, "x86_64");
    }

    #[test]
    fn mock_booted_updates_stream() {
        let status = mock_status().unwrap();
        let booted = booted_json(status).unwrap();
        assert_eq!(booted.base_metadata.stream, "testing-devel");
    }
}
