//! Asynchronous Cincinnati client.

// Cincinnati client.
mod client;
pub use client::Node;

#[cfg(test)]
mod mock_tests;

use crate::config::inputs;
use crate::identity::Identity;
use failure::{bail, Error, Fallible};
use futures::future;
use futures::prelude::*;
use prometheus::{IntCounter, IntGauge};
use serde::Serialize;

/// Metadata key for payload scheme.
pub static AGE_INDEX_KEY: &str = "org.fedoraproject.coreos.releases.age_index";

/// Metadata key for payload scheme.
pub static SCHEME_KEY: &str = "org.fedoraproject.coreos.scheme";

/// Metadata value for "checksum" payload scheme.
pub static CHECKSUM_SCHEME: &str = "checksum";

lazy_static::lazy_static! {
    static ref GRAPH_NODES: IntGauge = register_int_gauge!(opts!(
        "zincati_cincinnati_graph_nodes_count",
        "Number of nodes in Cincinnati update graph."
    )).unwrap();
    static ref UPDATE_CHECKS: IntCounter = register_int_counter!(opts!(
        "zincati_cincinnati_update_checks_total",
        "Total number of checks for updates to the upstream Cincinnati server."
    )).unwrap();
    static ref UPDATE_CHECKS_ERRORS: IntCounter = register_int_counter!(opts!(
        "zincati_cincinnati_update_checks_errors_total",
        "Total number of errors on checks for updates."
    )).unwrap();
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

        let c = Self { base_url };
        Ok(c)
    }

    /// Fetch next update-hint from Cincinnati.
    pub(crate) fn fetch_update_hint(
        &self,
        id: &Identity,
        can_check: bool,
    ) -> Box<Future<Item = Option<Node>, Error = ()>> {
        if !can_check {
            return Box::new(futures::future::ok(None));
        }

        let update = self
            .next_update(id)
            .inspect(|_| UPDATE_CHECKS.inc())
            .map_err(|e| {
                UPDATE_CHECKS_ERRORS.inc();
                log::error!("failed to check for updates: {}", e)
            });
        Box::new(update)
    }

    /// Get the next update.
    fn next_update(&self, id: &Identity) -> Box<Future<Item = Option<Node>, Error = Error>> {
        let params = id.cincinnati_params();
        let base_checksum = id.current_os.checksum.clone();
        let client = client::ClientBuilder::new(self.base_url.to_string())
            .query_params(Some(params))
            .build();

        let next = future::result(client)
            .and_then(|c| c.fetch_graph())
            .and_then(|graph| find_update(graph, base_checksum));
        Box::new(next)
    }
}

/// Walk the graph, looking for an update reachable from the given digest.
fn find_update(graph: client::Graph, digest: String) -> Fallible<Option<Node>> {
    GRAPH_NODES.set(graph.nodes.len() as i64);

    let cur_position = match graph
        .nodes
        .iter()
        .position(|n| is_same_checksum(n, &digest))
    {
        Some(pos) => pos,
        None => return Ok(None),
    };

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

    let mut updates = Vec::with_capacity(targets.len());
    for pos in targets {
        match graph.nodes.get(pos) {
            Some(n) => updates.push(n.clone()),
            None => bail!("target node '{}' not present in graph"),
        };
    }

    match updates.len() {
        0 => Ok(None),
        // TODO(lucab): stable pick next update
        _ => Ok(Some(updates.swap_remove(0))),
    }
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
}
