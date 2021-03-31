//! Interface to `rpm-ostree status --json`.

use super::actor::{RpmOstreeClient, StatusCache};
use super::Release;
use failure::{format_err, Fallible, ResultExt};
use filetime::FileTime;
use log::trace;
use prometheus::IntCounter;
use std::collections::BTreeSet;
use std::fs;
use std::rc::Rc;

use super::CLI_CLIENT;

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

impl From<&rpmostree_client::Deployment> for Release {
    fn from(d: &rpmostree_client::Deployment) -> Self {
        Release {
            checksum: d
                .base_checksum
                .clone()
                .unwrap_or_else(|| d.checksum.clone()),
            version: d.version.clone().unwrap_or_default(),
            age_index: None,
        }
    }
}

/// Parse local deployments from a status object.
fn parse_local_deployments(
    status: &rpmostree_client::Status,
    omit_staged: bool,
) -> Fallible<BTreeSet<Release>> {
    let mut deployments = BTreeSet::<Release>::new();
    for entry in &status.deployments {
        if omit_staged && entry.staged.unwrap_or_default() {
            continue;
        }

        deployments.insert(entry.into());
    }
    Ok(deployments)
}

/// Return local deployments, using client's cache if possible.
pub fn local_deployments(
    client: &mut RpmOstreeClient,
    omit_staged: bool,
) -> Fallible<BTreeSet<Release>> {
    let status = query_status(client)?;
    let local_depls = parse_local_deployments(&status, omit_staged)?;

    Ok(local_depls)
}

/// Ensure our status cache is up to date; if empty or out of date, run `rpm-ostree status` to populate it.
fn query_status_inner(client: &mut RpmOstreeClient) -> Fallible<Rc<rpmostree_client::Status>> {
    STATUS_CACHE_ATTEMPTS.inc();
    let ostree_depls_data = fs::metadata(OSTREE_DEPLS_PATH)
        .with_context(|e| format_err!("failed to query directory {}: {}", OSTREE_DEPLS_PATH, e))?;
    let ostree_depls_data_mtime = FileTime::from_last_modification_time(&ostree_depls_data);

    if let Some(cache) = &client.status_cache {
        if cache.mtime == ostree_depls_data_mtime {
            trace!("status cache is up to date");
            return Ok(cache.status.clone());
        }
    }

    STATUS_CACHE_MISSES.inc();
    trace!("cache stale, invoking rpm-ostree to retrieve local deployments");
    let status = Rc::new(
        rpmostree_client::query_status(&*CLI_CLIENT).map_err(failure::Error::from_boxed_compat)?,
    );
    client.status_cache = Some(StatusCache {
        status: Rc::clone(&status),
        mtime: ostree_depls_data_mtime,
    });

    Ok(status)
}

/// CLI executor for `rpm-ostree status --json`.
pub fn query_status(client: &mut RpmOstreeClient) -> Fallible<Rc<rpmostree_client::Status>> {
    RPM_OSTREE_STATUS_ATTEMPTS.inc();

    match query_status_inner(client) {
        Ok(s) => Ok(s),
        Err(e) => {
            RPM_OSTREE_STATUS_FAILURES.inc();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_status(path: &str) -> Fallible<rpmostree_client::Status> {
        let fp = std::fs::File::open(path).unwrap();
        let bufrd = std::io::BufReader::new(fp);
        let status = serde_json::from_reader(bufrd)?;
        Ok(status)
    }

    #[test]
    fn mock_deployments() {
        {
            let status = mock_status("tests/fixtures/rpm-ostree-status.json").unwrap();
            let deployments = parse_local_deployments(&status, false).unwrap();
            assert_eq!(deployments.len(), 1);
        }
        {
            let status = mock_status("tests/fixtures/rpm-ostree-staged.json").unwrap();
            let deployments = parse_local_deployments(&status, false).unwrap();
            assert_eq!(deployments.len(), 2);
        }
        {
            let status = mock_status("tests/fixtures/rpm-ostree-staged.json").unwrap();
            let deployments = parse_local_deployments(&status, true).unwrap();
            assert_eq!(deployments.len(), 1);
        }
    }
}
