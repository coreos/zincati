//! Interface to `rpm-ostree status --json`.

use super::actor::{RpmOstreeClient, StatusCache};
use super::Release;
use anyhow::{anyhow, bail, ensure, Context, Result};
use filetime::FileTime;
use log::{debug, trace};
use ostree_ext::container::OstreeImageReference;
use ostree_ext::oci_spec::distribution::Reference;
use prometheus::IntCounter;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::rc::Rc;

/// Path to local OSTree deployments. We use its mtime to check for modifications (e.g. new deployments)
/// to local deployments that might warrant querying `rpm-ostree status` again to update our knowledge
/// of the current state of deployments.
const OSTREE_DEPLS_PATH: &str = "/ostree/deploy";

/// Path to the fake deployment to use when migrating to OCI transport.
/// Using this fake deployment instead of the booted one, zincati
/// will get the next update from the OCI graph and rebase to the OCI image.
/// See https://github.com/coreos/fedora-coreos-tracker/issues/1823
static BOOTED_STATUS_OVERRIDE_FILE: &str = "/run/zincati/booted-status-override.json";

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
    container_image_reference_digest: Option<String>,
    base_checksum: Option<String>,
    base_commit_meta: BaseCommitMeta,
    checksum: String,
    // NOTE(lucab): missing field means "not staged".
    #[serde(default)]
    staged: bool,
    version: String,
}

#[derive(Clone, Debug, Deserialize)]
struct BaseCommitMeta {
    #[serde(rename = "ostree.manifest")]
    oci_manifest: Option<String>,
    #[serde(rename = "ostree.container.image-config")]
    oci_image_configuration: Option<String>,
}

impl Deployment {
    /// Convert into `Release`.
    pub fn into_release(self) -> Release {
        let payload = self.get_container_image_reference_digest().expect(
            "Failed to find OCI image reference. \n\
            Zincati only support bootable OCI containers and not ostree remotes.",
        );
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

    /// return the deployed container image reference
    /// e.g. ostree-remote-image:fedora:registry:quay.io/fedora/fedora-coreos:stable
    pub fn get_container_image_reference(&self) -> Option<OstreeImageReference> {
        self.container_image_reference
            .as_ref()
            .and_then(|s| s.as_str().try_into().ok())
    }

    /// Return the deployed container image as an oci image reference
    /// but with the digest instead of the tag
    /// e.g. quay.io/fedora/fedora-coreos@sha256:c4a15145a232d882ccf2ed32d22c06c01a7cf62317eb966a98340ae4bd56dfa6
    pub fn get_container_image_reference_digest(&self) -> Option<Reference> {
        match (
            &self.get_container_image_reference(),
            &self.container_image_reference_digest,
        ) {
            (Some(imgref), Some(digest)) => {
                let oci_ref: Option<Reference> = imgref.imgref.name.parse().ok();
                oci_ref.map(|reference| reference.clone_with_digest(digest.clone()))
            }
            _ => None,
        }
    }

    /// return the fedora-coreos update stream
    pub fn get_fcos_update_stream(&self) -> Result<String> {
        fedora_coreos_stream_from_deployment(self)
    }
}

/// Parse the booted deployment from status object.
pub fn parse_booted(status: &Status) -> Result<Release> {
    let status = booted_status(status)?;
    Ok(status.into_release())
}

fn fedora_coreos_stream_from_deployment(deploy: &Deployment) -> Result<String> {
    if deploy.base_commit_meta.oci_image_configuration.is_none()
        && deploy.base_commit_meta.oci_manifest.is_none()
    {
        bail!("Cannot deserialize ostree base image manifest");
    }

    // Check for `com.coreos.stream` label in OCI ImageConfiguration
    if let Some(oci_image_configuration) = deploy.base_commit_meta.oci_image_configuration.as_ref()
    {
        let image_configuration: ostree_ext::oci_spec::image::ImageConfiguration =
            serde_json::from_str(oci_image_configuration.as_str())?;
        if let Some(stream) = image_configuration.config().as_ref().and_then(|cfg| {
            cfg.labels()
                .as_ref()
                .and_then(|labels| labels.get("com.coreos.stream"))
        }) {
            ensure!(!stream.is_empty(), "empty stream value");
            debug!("Detected stream '{}' from com.coreos.stream label", stream);
            return Ok(stream.clone());
        }
    }

    // Fallback to `fedora-coreos.stream` annotation in OCI ImageManifest
    if let Some(oci_manifest) = deploy.base_commit_meta.oci_manifest.as_ref() {
        let manifest: ostree_ext::oci_spec::image::ImageManifest =
            serde_json::from_str(oci_manifest.as_str())?;
        if let Some(stream) = manifest
            .annotations()
            .as_ref()
            .and_then(|a| a.get("fedora-coreos.stream"))
        {
            ensure!(!stream.is_empty(), "empty stream value");
            debug!(
                "Detected stream '{}' from fedora-coreos.stream annotation",
                stream
            );
            return Ok(stream.clone());
        }
    }

    Err(anyhow!(
        "Missing `com.coreos.stream` label in the base image configuration,
        or `fedora-coreos.stream` annotation in the base image manifest"
    ))
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
    let staged = get_staged_deployment(status);

    match staged {
        None => Ok(None),
        Some(json) => {
            let stream = fedora_coreos_stream_from_deployment(&json)?;
            let release = json.into_release();
            Ok(Some((release, stream)))
        }
    }
}

/// Return the pending/staged deployment
pub fn get_staged_deployment(status: &Status) -> Option<Deployment> {
    // There can be at most one staged/pending rpm-ostree deployment,
    // thus we only consider the first matching entry (if any).
    status.deployments.iter().find(|d| d.staged).cloned()
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
    let mut status: Status = serde_json::from_slice(&cmdrun.stdout)?;

    // if the oci_migration file exist we want to graft it into the
    // output of rpm-ostree status.
    // Replace the booted status with the content of the override file
    let status_override_file = std::path::Path::new(BOOTED_STATUS_OVERRIDE_FILE);
    if status_override_file.exists() {
        let rdr = std::fs::File::open(status_override_file).map(std::io::BufReader::new)?;
        let override_boot_depl: Deployment = serde_json::from_reader(rdr)?;

        // Keep the actual status info and just replace the booted deployement.
        // We need other deployements info to know if we rollbacked
        // or if a deployment is staged.
        status.deployments = status
            .deployments
            .into_iter()
            .map(|d| {
                if d.booted {
                    override_boot_depl.clone()
                } else {
                    d
                }
            })
            .collect();
    }
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
            assert_eq!(deployments.len(), 2);
        }
        {
            let status = mock_status("tests/fixtures/rpm-ostree-status-annotation.json").unwrap();
            let deployments = parse_local_deployments(&status, false);
            assert_eq!(deployments.len(), 1);
        }
    }

    #[test]
    fn mock_booted_updates_stream() {
        {
            let status = mock_status("tests/fixtures/rpm-ostree-status.json").unwrap();
            let booted = booted_status(&status).unwrap();
            let stream = fedora_coreos_stream_from_deployment(&booted).unwrap();
            assert_eq!(stream, "stable");
        }
        {
            let status = mock_status("tests/fixtures/rpm-ostree-status-annotation.json").unwrap();
            let booted = booted_status(&status).unwrap();
            let stream = fedora_coreos_stream_from_deployment(&booted).unwrap();
            assert_eq!(stream, "stable");
        }
    }

    #[test]
    fn mock_booted_oci_deployment() {
        let status = mock_status("tests/fixtures/rpm-ostree-status.json").unwrap();
        let booted = booted_status(&status).unwrap();
        let stream = fedora_coreos_stream_from_deployment(&booted).unwrap();
        assert_eq!(stream, "stable");
        let img_ref = booted.get_container_image_reference();
        assert!(img_ref.is_some());
        let img_ref = img_ref.unwrap();
        assert_eq!(img_ref.sigverify, SignatureSource::ContainerPolicy);
        assert_eq!(
            img_ref.imgref.name,
            "quay.io/fedora/fedora-coreos:stable".to_string()
        );
        let imgref_with_digest = booted.get_container_image_reference_digest();
        assert!(imgref_with_digest.is_some());
        let imgref_with_digest = imgref_with_digest.unwrap();
        assert_eq!(imgref_with_digest.to_string(), "quay.io/fedora/fedora-coreos@sha256:ca99893c80a7b84dd84d4143bd27538207c2f38ab6647a58d9c8caa251f9a087".to_string());
    }
}
