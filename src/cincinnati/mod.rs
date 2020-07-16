//! Asynchronous Cincinnati client.

// Cincinnati client.
mod client;
pub use client::{CincinnatiError, Node};

#[cfg(test)]
mod mock_tests;

use crate::config::inputs;
use crate::identity::Identity;
use crate::rpm_ostree::Release;
use failure::{bail, Fallible, ResultExt};
use futures::prelude::*;
use futures::TryFutureExt;
use prometheus::{IntCounter, IntCounterVec, IntGauge};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::fs::Permissions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::pin::Pin;

/// Metadata key for payload scheme.
pub static AGE_INDEX_KEY: &str = "org.fedoraproject.coreos.releases.age_index";

/// Metadata key for payload scheme.
pub static SCHEME_KEY: &str = "org.fedoraproject.coreos.scheme";

/// Metadata key for dead-end sentinel.
pub static DEADEND_KEY: &str = "org.fedoraproject.coreos.updates.deadend";

/// Metadata key for dead-end reason.
pub static DEADEND_REASON_KEY: &str = "org.fedoraproject.coreos.updates.deadend_reason";

/// Metadata value for "checksum" payload scheme.
pub static CHECKSUM_SCHEME: &str = "checksum";

lazy_static::lazy_static! {
    static ref GRAPH_NODES: IntGauge = register_int_gauge!(opts!(
        "zincati_cincinnati_graph_nodes_count",
        "Number of nodes in Cincinnati update graph."
    )).unwrap();
    static ref GRAPH_EDGES: IntGauge = register_int_gauge!(opts!(
        "zincati_cincinnati_graph_edges_count",
        "Number of edges in Cincinnati update graph."
    )).unwrap();
    static ref BOOTED_DEADEND: IntGauge = register_int_gauge!(
        "zincati_cincinnati_booted_release_is_deadend",
        "Whether currently booted OS release is a dead-end."
    ).unwrap();
    static ref UPDATE_CHECKS: IntCounter = register_int_counter!(opts!(
        "zincati_cincinnati_update_checks_total",
        "Total number of checks for updates to the upstream Cincinnati server."
    )).unwrap();
    static ref UPDATE_CHECKS_ERRORS: IntCounterVec = register_int_counter_vec!(
        "zincati_cincinnati_update_checks_errors_total",
        "Total number of errors while checking for updates.",
        &["kind"]
    ).unwrap();
}

/// Cincinnati configuration.
#[derive(Debug, Serialize)]
pub struct Cincinnati {
    /// Service base URL.
    pub base_url: String,
}

impl Cincinnati {
    /// Process Cincinnati configuration.
    pub(crate) fn with_config(cfg: inputs::CincinnatiInput, id: &Identity) -> Fallible<Self> {
        if cfg.base_url.is_empty() {
            bail!("empty Cincinnati base URL");
        }

        // Substitute templated key with agent runtime values.
        let base_url = if envsubst::is_templated(&cfg.base_url) {
            let context = id.url_variables();
            envsubst::validate_vars(&context)?;
            envsubst::substitute(cfg.base_url, &context)?
        } else {
            cfg.base_url
        };
        log::info!("Cincinnati service: {}", &base_url);

        let c = Self { base_url };
        Ok(c)
    }

    /// Fetch next update-hint from Cincinnati.
    pub(crate) fn fetch_update_hint(
        &self,
        id: &Identity,
        deployments: BTreeSet<Release>,
        allow_downgrade: bool,
    ) -> Pin<Box<dyn Future<Output = Option<Release>>>> {
        UPDATE_CHECKS.inc();
        log::trace!("checking upstream Cincinnati server for updates");

        let update = self
            .next_update(id, deployments, allow_downgrade)
            .unwrap_or_else(|e| {
                UPDATE_CHECKS_ERRORS
                    .with_label_values(&[&e.error_kind()])
                    .inc();
                log::error!("failed to check Cincinnati for updates: {}", e);
                None
            });
        Box::pin(update)
    }

    /// Get the next update.
    fn next_update(
        &self,
        id: &Identity,
        deployments: BTreeSet<Release>,
        allow_downgrade: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Release>, CincinnatiError>>>> {
        let booted = id.current_os.clone();
        let params = id.cincinnati_params();
        let client = client::ClientBuilder::new(self.base_url.to_string())
            .query_params(Some(params))
            .build()
            .map_err(|e| CincinnatiError::FailedClientBuilder(e.to_string()));

        let next = futures::future::ready(client)
            .and_then(|c| c.fetch_graph())
            .and_then(move |graph| async move {
                find_update(graph, booted, deployments, allow_downgrade)
            });
        Box::pin(next)
    }
}

/// Write MOTD indicating a dead-end release, with the passed `reason`.
/// If `reason` is `None`, the MOTD written is left as an empty file.
fn refresh_deadend_motd(reason: Option<String>) -> Fallible<()> {
    // Avoid showing partially-written messages using tempfile and
    // persist (rename).
    let mut f = tempfile::Builder::new()
        .prefix(".deadend.")
        .suffix(".motd.partial")
        // Create the tempfile in the same directory as the final MOTD,
        // to ensure proper SELinux labels are applied to the tempfile
        // before renaming.
        .tempfile_in("/run/zincati/public/motd.d")
        .with_context(|e| format!("failed to create temporary MOTD file: {}", e))?;
    // Set correct permissions of the temporary file, before moving to
    // the destination (`tempfile` creates files with mode 0600).
    fs::set_permissions(f.path(), Permissions::from_mode(0o664))
        .with_context(|e| format!("failed to set permissions of temporary MOTD file: {}", e))?;

    if let Some(reason) = reason {
        writeln!(
            f,
            "This release is a dead-end and won't auto-update: {}",
            reason
        )
        .with_context(|e| format!("failed to write MOTD: {}", e))?;
    }

    f.persist("/run/zincati/public/motd.d/deadend.motd")
        .with_context(|e| format!("failed to persist temporary MOTD file: {}", e))?;
    Ok(())
}

/// Evaluate and record whether booted OS is a dead-end release, and
/// log that information in a MOTD file.
fn refresh_deadend_status(node: &Node) {
    let deadend_reason = evaluate_deadend(node);
    match &deadend_reason {
        Some(reason) => {
            log::info!("dead-end release detected: {}", reason);
            BOOTED_DEADEND.set(1);
        }
        None => {
            BOOTED_DEADEND.set(0);
        }
    };
    if let Err(e) = refresh_deadend_motd(deadend_reason) {
        log::warn!("failed to update dead-end release MOTD: {}", e);
    }
}

/// Walk the graph, looking for an update reachable from the given digest.
fn find_update(
    graph: client::Graph,
    booted_depl: Release,
    local_depls: BTreeSet<Release>,
    allow_downgrade: bool,
) -> Result<Option<Release>, CincinnatiError> {
    GRAPH_NODES.set(graph.nodes.len() as i64);
    GRAPH_EDGES.set(graph.edges.len() as i64);
    log::trace!(
        "got an update graph with {} nodes and {} edges",
        graph.nodes.len(),
        graph.edges.len()
    );

    // Find booted deployment in graph.
    let (cur_position, cur_node) = match graph
        .nodes
        .iter()
        .enumerate()
        .find(|(_, node)| is_same_checksum(node, &booted_depl.checksum))
    {
        Some(current) => current,
        None => return Ok(None),
    };
    drop(booted_depl);
    let cur_release = Release::from_cincinnati(cur_node.clone())
        .map_err(|e| CincinnatiError::FailedNodeParsing(e.to_string()))?;

    refresh_deadend_status(&cur_node);

    // Try to find all local deployments in the graph too.
    let local_releases = find_local_releases(&graph, local_depls);

    // Find all possible update targets from booted deployment.
    let targets: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|(src, dst)| {
            if *src == cur_position as u64 {
                Some(*dst as usize)
            } else {
                None
            }
        })
        .collect();
    let mut updates = BTreeSet::new();
    for pos in targets {
        let node = match graph.nodes.get(pos) {
            Some(n) => n.clone(),
            None => {
                let msg = format!("target node '{}' not present in graph", pos);
                return Err(CincinnatiError::FailedNodeLookup(msg));
            }
        };
        let release = Release::from_cincinnati(node)
            .map_err(|e| CincinnatiError::FailedNodeParsing(e.to_string()))?;
        updates.insert(release);
    }

    // Exclude target already deployed locally in the past.
    let new_updates = updates.difference(&local_releases);

    // Pick highest available updates target (based on age-index).
    let next = match new_updates.last().cloned() {
        Some(rel) => rel,
        None => return Ok(None),
    };

    // Check for downgrades.
    if next <= cur_release {
        log::warn!("downgrade hint towards target release '{}'", next.version);
        if !allow_downgrade {
            log::warn!("update hint rejected, downgrades are not allowed by configuration");
            return Ok(None);
        }
    }

    Ok(Some(next))
}

/// Try to match a set of (local) deployments to their graph entries.
fn find_local_releases(graph: &client::Graph, depls: BTreeSet<Release>) -> BTreeSet<Release> {
    use std::collections::HashSet;

    let mut local_releases = BTreeSet::new();
    let checksums: HashSet<String> = depls.into_iter().map(|rel| rel.checksum).collect();

    for entry in &graph.nodes {
        if !checksums.contains(&entry.payload) {
            continue;
        }

        if let Ok(release) = Release::from_cincinnati(entry.clone()) {
            local_releases.insert(release);
        }
    }

    local_releases
}

/// Check whether input node matches current checksum.
fn is_same_checksum(node: &Node, checksum: &str) -> bool {
    let payload_is_checksum = node
        .metadata
        .get(SCHEME_KEY)
        .map(|v| v == CHECKSUM_SCHEME)
        .unwrap_or(false);

    payload_is_checksum && node.payload == checksum
}

/// Check and record whether input node is a dead-end.
///
/// Note: this is usually only called on the node
/// corresponding to the booted deployment.
fn evaluate_deadend(node: &Node) -> Option<String> {
    let node_is_deadend = node
        .metadata
        .get(DEADEND_KEY)
        .map(|v| v == "true")
        .unwrap_or(false);

    if !node_is_deadend {
        return None;
    }

    let mut deadend_reason = node
        .metadata
        .get(DEADEND_REASON_KEY)
        .map(|v| v.to_string())
        .unwrap_or_default();
    if deadend_reason.is_empty() {
        deadend_reason = "(unknown reason)".to_string();
    }

    Some(deadend_reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn source_node_comparison() {
        let current = "current-sha";

        let mut metadata = HashMap::new();
        metadata.insert(SCHEME_KEY.to_string(), CHECKSUM_SCHEME.to_string());
        let matching = Node {
            version: "v0".to_string(),
            payload: current.to_string(),
            metadata,
        };
        assert!(is_same_checksum(&matching, current));

        let mismatch = Node {
            version: "v0".to_string(),
            payload: "mismatch".to_string(),
            metadata: HashMap::new(),
        };
        assert!(!is_same_checksum(&mismatch, current));
    }

    #[test]
    fn deadend_node() {
        let deadend_json = r#"
{
  "version": "30.20190716.1",
  "metadata": {
    "org.fedoraproject.coreos.releases.age_index": "0",
    "org.fedoraproject.coreos.scheme": "checksum",
    "org.fedoraproject.coreos.updates.deadend": "true",
    "org.fedoraproject.coreos.updates.deadend_reason": "https://github.com/coreos/fedora-coreos-tracker/issues/215"
  },
  "payload": "ff4803b069b5a10e5bee2f6bb0027117637559d813c2016e27d57b309dd09d6f"
}
"#;
        let deadend: Node = serde_json::from_str(deadend_json).unwrap();
        let reason = "https://github.com/coreos/fedora-coreos-tracker/issues/215".to_string();
        assert_eq!(evaluate_deadend(&deadend), Some(reason));

        let common_json = r#"
{
  "version": "30.20190725.0",
  "metadata": {
    "org.fedoraproject.coreos.releases.age_index": "1",
    "org.fedoraproject.coreos.scheme": "checksum"
  },
  "payload": "8b79877efa7ac06becd8637d95f8ca83aa385f89f383288bf3c2c31ca53216c7"
}
"#;

        let common: Node = serde_json::from_str(common_json).unwrap();
        assert_eq!(evaluate_deadend(&common), None);
    }
}
