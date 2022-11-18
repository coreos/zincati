//! Interface to `rpm-ostree status --json`.

use super::actor::{RpmOstreeClient, StatusCache};
use super::Release;
use anyhow::{anyhow, ensure, Context, Result};
use filetime::FileTime;
use log::trace;
use prometheus::IntCounter;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::rc::Rc;

/// The well-known Fedora CoreOS base image.
const FEDORA_COREOS_CONTAINER: &str = "quay.io/fedora/fedora-coreos";

/// Path to local OSTree deployments. We use its mtime to check for modifications (e.g. new deployments)
/// to local deployments that might warrant querying `rpm-ostree status` again to update our knowledge
/// of the current state of deployments.
const OSTREE_DEPLS_PATH: &str = "/ostree/deploy";

lazy_static::lazy_static! {
    static ref STATUS_CACHE_ATTEMPTS: IntCounter = register_int_counter!(opts!(
        "zincati_rpm_ostree_status_cache_requests_total",
        "Total number of attempts to query rpm-ostree actor's cached status."
    )).unwrap();
    static ref STATUS_CACHE_MISSES: IntCounter = register_int_counter!(opts!(
        "zincati_rpm_ostree_status_cache_misses_total",
        "Total number of times rpm-ostree actor's cached status is stale during queries."
    )).unwrap();
    // This is not equivalent to `zincati_rpm_ostree_status_cache_misses_total` as there
    // are cases where `rpm-ostree status` is called directly without checking the cache.
    static ref RPM_OSTREE_STATUS_ATTEMPTS: IntCounter = register_int_counter!(opts!(
        "zincati_rpm_ostree_status_attempts_total",
        "Total number of 'rpm-ostree status' attempts."
    )).unwrap();
    static ref RPM_OSTREE_STATUS_FAILURES: IntCounter = register_int_counter!(opts!(
        "zincati_rpm_ostree_status_failures_total",
        "Total number of 'rpm-ostree status' failures."
    )).unwrap();
}

/// JSON output from `rpm-ostree status --json`
#[derive(Clone, Debug, Deserialize)]
pub struct StatusJson {
    deployments: Vec<DeploymentJson>,
}

/// Partial deployment object (only fields relevant to zincati).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DeploymentJson {
    booted: bool,
    container_image_reference: Option<String>,
    base_checksum: Option<String>,
    #[serde(rename = "base-commit-meta")]
    base_metadata: BaseCommitMetaJson,
    checksum: String,
    // NOTE(lucab): missing field means "not staged".
    #[serde(default)]
    staged: bool,
    version: String,
}

/// Metadata from base commit (only fields relevant to zincati).
#[derive(Clone, Debug, Deserialize)]
struct BaseCommitMetaJson {
    #[serde(rename = "fedora-coreos.stream")]
    stream: Option<String>,
}

impl DeploymentJson {
    /// Convert into `Release`.
    pub fn into_release(self) -> Release {
        Release {
            checksum: self.base_revision(),
            version: self.version,
            age_index: None,
        }
    }

    /// Return the deployment base revision.
    pub fn base_revision(&self) -> String {
        self.base_checksum
            .clone()
            .unwrap_or_else(|| self.checksum.clone())
    }
}

/// Parse the booted deployment from status object.
pub fn parse_booted(status: &StatusJson) -> Result<Release> {
    let json = booted_json(status)?;
    Ok(json.into_release())
}

fn fedora_coreos_stream_from_deployment(deploy: &DeploymentJson) -> Result<String> {
    if let Some(cr) = deploy.container_image_reference.as_deref() {
        let cr = super::imageref::OstreeImageReference::try_from(cr)
            .with_context(|| format!("Failed to parse container image reference {cr}"))?;
        let ir = &cr.imgref;
        let tx = ir.transport;
        if tx != super::imageref::Transport::Registry {
            anyhow::bail!("Unhandled container transport {tx}");
        }
        let name = ir.name.as_str();
        let (name, tag) = name
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("Failed to find tag in {name}"))?;
        if name != FEDORA_COREOS_CONTAINER {
            anyhow::bail!("Unhandled container image {name}");
        }
        ensure!(!tag.is_empty(), "empty tag value");
        Ok(tag.to_string())
    } else {
        let stream = deploy.base_metadata.stream.as_deref().ok_or_else(|| {
            anyhow!("Failed to find Fedora CoreOS stream metadata from commit object")
        })?;
        ensure!(!stream.is_empty(), "empty stream value");
        Ok(stream.to_string())
    }
}

/// Parse updates stream for booted deployment from status object.
pub fn parse_booted_updates_stream(status: &StatusJson) -> Result<String> {
    let json = booted_json(status)?;
    fedora_coreos_stream_from_deployment(&json)
}

/// Parse pending deployment from status object.
pub fn parse_pending_deployment(status: &StatusJson) -> Result<Option<(Release, String)>> {
    // There can be at most one staged/pending rpm-ostree deployment,
    // thus we only consider the first matching entry (if any).
    let staged = status.deployments.iter().find(|d| d.staged).cloned();

    match staged {
        None => Ok(None),
        Some(json) => {
            let stream = fedora_coreos_stream_from_deployment(&json)?;
            let release = json.into_release();
            Ok(Some((release, stream)))
        }
    }
}

/// Parse local deployments from a status object.
fn parse_local_deployments(status: &StatusJson, omit_staged: bool) -> BTreeSet<Release> {
    let mut deployments = BTreeSet::<Release>::new();
    for entry in &status.deployments {
        if omit_staged && entry.staged {
            continue;
        }

        let release = entry.clone().into_release();
        deployments.insert(release);
    }
    deployments
}

/// Return local deployments, using client's cache if possible.
pub fn local_deployments(
    client: &mut RpmOstreeClient,
    omit_staged: bool,
) -> Result<BTreeSet<Release>> {
    let status = status_json(client)?;
    let local_depls = parse_local_deployments(&status, omit_staged);

    Ok(local_depls)
}

/// Return JSON object for booted deployment.
fn booted_json(status: &StatusJson) -> Result<DeploymentJson> {
    let booted = status
        .clone()
        .deployments
        .into_iter()
        .find(|d| d.booted)
        .ok_or_else(|| anyhow!("no booted deployment found"))?;

    ensure!(!booted.base_revision().is_empty(), "empty base revision");
    ensure!(!booted.version.is_empty(), "empty version");
    Ok(booted)
}

/// Ensure our status cache is up to date; if empty or out of date, run `rpm-ostree status` to populate it.
fn status_json(client: &mut RpmOstreeClient) -> Result<Rc<StatusJson>> {
    STATUS_CACHE_ATTEMPTS.inc();
    let ostree_depls_data = fs::metadata(OSTREE_DEPLS_PATH)
        .with_context(|| format!("failed to query directory {}", OSTREE_DEPLS_PATH))?;
    let ostree_depls_data_mtime = FileTime::from_last_modification_time(&ostree_depls_data);

    if let Some(cache) = &client.status_cache {
        if cache.mtime == ostree_depls_data_mtime {
            trace!("status cache is up to date");
            return Ok(cache.status.clone());
        }
    }

    STATUS_CACHE_MISSES.inc();
    trace!("cache stale, invoking rpm-ostree to retrieve local deployments");
    let status = Rc::new(invoke_cli_status(false)?);
    client.status_cache = Some(StatusCache {
        status: Rc::clone(&status),
        mtime: ostree_depls_data_mtime,
    });

    Ok(status)
}

/// CLI executor for `rpm-ostree status --json`.
pub fn invoke_cli_status(booted_only: bool) -> Result<StatusJson> {
    RPM_OSTREE_STATUS_ATTEMPTS.inc();

    let mut cmd = std::process::Command::new("rpm-ostree");
    cmd.arg("status").env("RPMOSTREE_CLIENT_ID", "zincati");

    // Try to request the minimum scope we need.
    if booted_only {
        cmd.arg("--booted");
    }

    let cmdrun = cmd
        .arg("--json")
        .output()
        .context("failed to run 'rpm-ostree' binary")?;

    if !cmdrun.status.success() {
        RPM_OSTREE_STATUS_FAILURES.inc();
        anyhow::bail!(
            "rpm-ostree status failed:\n{}",
            String::from_utf8_lossy(&cmdrun.stderr)
        );
    }
    let status: StatusJson = serde_json::from_slice(&cmdrun.stdout)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_status(path: &str) -> Result<StatusJson> {
        let fp = std::fs::File::open(path).unwrap();
        let bufrd = std::io::BufReader::new(fp);
        let status: StatusJson = serde_json::from_reader(bufrd)?;
        Ok(status)
    }

    #[test]
    fn mock_deployments() {
        {
            let status = mock_status("tests/fixtures/rpm-ostree-status.json").unwrap();
            let deployments = parse_local_deployments(&status, false);
            assert_eq!(deployments.len(), 1);
        }
        {
            let status = mock_status("tests/fixtures/rpm-ostree-staged.json").unwrap();
            let deployments = parse_local_deployments(&status, false);
            assert_eq!(deployments.len(), 2);
        }
        {
            let status = mock_status("tests/fixtures/rpm-ostree-staged.json").unwrap();
            let deployments = parse_local_deployments(&status, true);
            assert_eq!(deployments.len(), 1);
        }
    }

    #[test]
    fn mock_booted_updates_stream() {
        let status = mock_status("tests/fixtures/rpm-ostree-status.json").unwrap();
        let booted = booted_json(&status).unwrap();
        let stream = fedora_coreos_stream_from_deployment(&booted).unwrap();
        assert_eq!(stream, "testing-devel");
    }
}
