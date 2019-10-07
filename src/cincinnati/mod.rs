//! Asynchronous Cincinnati client.

// Cincinnati client.
mod client;
pub use client::{CincinnatiError, Node};

#[cfg(test)]
mod mock_tests;

use crate::config::inputs;
use crate::identity::Identity;
use crate::rpm_ostree::Release;
use failure::{bail, Fallible};
use futures::future;
use futures::prelude::*;
use prometheus::{IntCounter, IntCounterVec, IntGauge};
use serde::Serialize;
use std::collections::BTreeSet;

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
        can_check: bool,
    ) -> Box<dyn Future<Item = Option<Release>, Error = ()>> {
        if !can_check {
            return Box::new(futures::future::ok(None));
        }

        UPDATE_CHECKS.inc();
        log::trace!("checking upstream Cincinnati server for updates");

        let update = self.next_update(id, deployments).map_err(|e| {
            UPDATE_CHECKS_ERRORS
                .with_label_values(&[&e.error_kind()])
                .inc();
            log::error!("failed to check Cincinnati for updates: {}", e)
        });
        Box::new(update)
    }

    /// Get the next update.
    fn next_update(
        &self,
        id: &Identity,
        deployments: BTreeSet<Release>,
    ) -> Box<dyn Future<Item = Option<Release>, Error = CincinnatiError>> {
        let booted = id.current_os.clone();
        let params = id.cincinnati_params();
        let client = client::ClientBuilder::new(self.base_url.to_string())
            .query_params(Some(params))
            .build()
            .map_err(|e| CincinnatiError::FailedClientBuilder(e.to_string()));

        let next = future::result(client)
            .and_then(|c| c.fetch_graph())
            .and_then(move |graph| find_update(graph, booted, deployments));
        Box::new(next)
    }
}

/// Walk the graph, looking for an update reachable from the given digest.
fn find_update(
    graph: client::Graph,
    booted_depl: Release,
    local_depls: BTreeSet<Release>,
) -> Result<Option<Release>, CincinnatiError> {
    GRAPH_NODES.set(graph.nodes.len() as i64);
    GRAPH_EDGES.set(graph.edges.len() as i64);
    log::trace!(
        "got an update graph with {} nodes and {} edges",
        graph.nodes.len(),
        graph.edges.len()
    );

    // Find booted deployment in graph.
    let (cur_position, cur_release) = match graph
        .nodes
        .iter()
        .enumerate()
        .find(|(_, node)| is_same_checksum(node, &booted_depl.checksum))
    {
        Some(current) => current,
        None => return Ok(None),
    };
    drop(booted_depl);

    // Evaluate and record whether booted OS is a dead-end release.
    // TODO(lucab): consider exposing this information in more places
    // (e.g. logs, motd, env/json file in a well-known location).
    let is_deadend = match evaluate_deadend(&cur_release) {
        Some(_) => 1,
        None => 0,
    };
    BOOTED_DEADEND.set(is_deadend);

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
