//! Interface to `rpm-ostree status --json`.

use super::actor::{RpmOstreeClient, StatusCache};
use super::{Payload, Release};
use anyhow::{anyhow, bail, ensure, Context, Result};
use filetime::FileTime;
use log::trace;
use ostree_ext::container::OstreeImageReference;
use prometheus::IntCounter;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::rc::Rc;

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

/// An error which should not result in a retry/restart.
#[derive(Debug, Clone)]
pub struct SystemInoperable(String);

impl std::fmt::Display for SystemInoperable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for SystemInoperable {}

/// JSON output from `rpm-ostree status --json`
#[derive(Clone, Debug, Deserialize)]
pub struct Status {
    deployments: Vec<Deployment>,
}

/// Partial deployment object (only fields relevant to zincati).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Deployment {
    booted: bool,
    container_image_reference: Option<String>,
    custom_origin: Option<CustomOrigin>,
    base_checksum: Option<String>,
    base_commit_meta: BaseCommitMeta,
    checksum: String,
    // NOTE(lucab): missing field means "not staged".
    #[serde(default)]
    staged: bool,
    version: String,
}

/// Custom origin fields
#[derive(Clone, Debug, Deserialize)]
pub struct CustomOrigin {
    pub url: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
struct BaseCommitMeta {
    #[serde(rename = "fedora-coreos.stream")]
    stream: Option<String>,
    #[serde(rename = "ostree.manifest")]
    oci_manifest: Option<String>,
}

impl Deployment {
    /// Convert into `Release`.
    pub fn into_release(self) -> Release {
        let payload = if let Some(image) = self.container_image_reference {
            Payload::Pullspec(image)
        } else {
            Payload::Checksum(self.base_revision())
        };
        Release {
            payload,
            version: self.version,
            age_index: None,
        }
    }

    /// Return the deployment base revision.
    pub fn base_revision(&self) -> String {
        self.container_image_reference
            .clone()
            .or(self.base_checksum.clone())
            .unwrap_or_else(|| self.checksum.clone())
    }

    /// return the custom origin fields
    pub fn custom_origin(&self) -> Option<CustomOrigin> {
        self.custom_origin.clone()
    }

    /// return the deployed container image reference
    pub fn container_image_reference(&self) -> Option<OstreeImageReference> {
        self.container_image_reference
            .as_ref()
            .and_then(|s| s.as_str().try_into().ok())
    }
}

/// Parse the booted deployment from status object.
pub fn parse_booted(status: &Status) -> Result<Release> {
    let status = booted_status(status)?;
    Ok(status.into_release())
}

fn fedora_coreos_stream_from_deployment(deploy: &Deployment) -> Result<String> {
    let stream = match (
        deploy.base_commit_meta.stream.clone(),
        deploy.base_commit_meta.oci_manifest.clone(),
    ) {
        (Some(stream), _) => stream.clone(), // in the OCI case, base commit meta is an escaped JSON string of
        // an OCI ImageManifest. Deserialize it properly.
        (_, Some(oci_manifest)) => {
            let manifest: oci_spec::image::ImageManifest =
                serde_json::from_str(oci_manifest.as_str())?;
            manifest
                .annotations()
                .clone()
                .and_then(|a| a.get("fedora-coreos.stream").cloned())
                .ok_or_else(|| {
                    anyhow!("Missing `fedora-coreos.stream` in base image manifest annotations")
                })?
        }
        (None, None) => bail!("Cannot deserialize ostree base image manifest"),
    };
    ensure!(!stream.is_empty(), "empty stream value");
    Ok(stream)
}

/// Parse updates stream for booted deployment from status object.
pub fn parse_booted_updates_stream(status: &Status) -> Result<String> {
    let json = booted_status(status)?;
    fedora_coreos_stream_from_deployment(&json)
}

/// Parse pending deployment from status object.
pub fn parse_pending_deployment(status: &Status) -> Result<Option<(Release, String)>> {
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
fn parse_local_deployments(status: &Status, omit_staged: bool) -> BTreeSet<Release> {
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
    let status = get_status(client)?;
    let local_depls = parse_local_deployments(&status, omit_staged);

    Ok(local_depls)
}

/// Return JSON object for booted deployment.
pub fn booted_status(status: &Status) -> Result<Deployment> {
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
fn get_status(client: &mut RpmOstreeClient) -> Result<Rc<Status>> {
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
pub fn invoke_cli_status(booted_only: bool) -> Result<Status> {
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
    let status: Status = serde_json::from_slice(&cmdrun.stdout)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use ostree_ext::container::SignatureSource;

    use super::*;

    fn mock_status(path: &str) -> Result<Status> {
        let r = std::fs::File::open(path).map(std::io::BufReader::new)?;
        Ok(serde_json::from_reader(r)?)
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
        {
            let status = mock_status("tests/fixtures/rpm-ostree-oci-status.json").unwrap();
            let deployments = parse_local_deployments(&status, false);
            assert_eq!(deployments.len(), 1);
        }
    }

    #[test]
    fn mock_booted_updates_stream() {
        let status = mock_status("tests/fixtures/rpm-ostree-status.json").unwrap();
        let booted = booted_status(&status).unwrap();
        let stream = fedora_coreos_stream_from_deployment(&booted).unwrap();
        assert_eq!(stream, "testing-devel");
    }

    #[test]
    fn mock_booted_oci_deployment() {
        let status = mock_status("tests/fixtures/rpm-ostree-oci-status.json").unwrap();
        let booted = booted_status(&status).unwrap();
        let stream = fedora_coreos_stream_from_deployment(&booted).unwrap();
        assert_eq!(stream, "testing");
        let img_ref = booted.container_image_reference();
        assert!(img_ref.is_some());
        let img_ref = img_ref.unwrap();
        assert_eq!(
            img_ref.sigverify,
            SignatureSource::OstreeRemote("fedora".to_string())
        );
        assert_eq!(img_ref.imgref.name, "quay.io/fedora/fedora-coreos@sha256:c4a15145a232d882ccf2ed32d22c06c01a7cf62317eb966a98340ae4bd56dfa6".to_string());

        let custom_origin = booted.custom_origin();
        assert!(custom_origin.is_some());
        let custom_origin = custom_origin.unwrap();
        assert_eq!(
            custom_origin.url,
            "quay.io/fedora/fedora-coreos@sha256:c4a15145a232d882ccf2ed32d22c06c01a7cf62317eb966a98340ae4bd56dfa6"
        );
        assert_eq!(
            custom_origin.description,
            "Fedora CoreOS Testing stream through OCI images"
        );
    }
}
