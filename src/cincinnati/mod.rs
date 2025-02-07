//! Asynchronous Cincinnati client.

// Cincinnati client.
mod client;
pub use client::{CincinnatiError, Node};

#[cfg(test)]
mod mock_tests;

use crate::config::inputs;
use crate::identity::Identity;
use crate::rpm_ostree::{Payload, Release};
use anyhow::{Context, Result};
use fn_error_context::context;
use futures::prelude::*;
use futures::TryFutureExt;
use ostree_ext::container::OstreeImageReference;
use prometheus::{IntCounter, IntCounterVec, IntGauge};
use serde::Serialize;
use std::collections::BTreeSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};

/// Metadata key for payload scheme.
pub static AGE_INDEX_KEY: &str = "org.fedoraproject.coreos.releases.age_index";

/// Metadata key for payload scheme.
pub static SCHEME_KEY: &str = "org.fedoraproject.coreos.scheme";

/// Metadata key for dead-end sentinel.
pub static DEADEND_KEY: &str = "org.fedoraproject.coreos.updates.deadend";

/// Metadata key for dead-end reason.
pub static DEADEND_REASON_KEY: &str = "org.fedoraproject.coreos.updates.deadend_reason";

/// Metadata value for "checksum" payload scheme.
pub const CHECKSUM_SCHEME: &str = "checksum";

/// Metadata value for "oci" payload scheme.
pub const OCI_SCHEME: &str = "oci";

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
    static ref UPDATE_TARGETS_IGNORED: IntGauge = register_int_gauge!(
        "zincati_cincinnati_ignored_update_targets",
        "Number of ignored targets among update targets found."
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
    static ref DEADEND_STATE : DeadEndState = DeadEndState::default();
}

/// For tracking a dead-end release.
pub struct DeadEndState(AtomicU8);

impl Default for DeadEndState {
    fn default() -> Self {
        Self(AtomicU8::new(DeadEndState::UNKNOWN))
    }
}

impl DeadEndState {
    const FALSE: u8 = 0;
    const TRUE: u8 = 1;
    const UNKNOWN: u8 = 2;

    /// Return whether this is in a known dead-end state.
    pub fn is_deadend(&self) -> bool {
        self.0.load(Ordering::SeqCst) == Self::TRUE
    }

    /// Return whether this is in a known NOT dead-end state.
    pub fn is_no_deadend(&self) -> bool {
        self.0.load(Ordering::SeqCst) == Self::FALSE
    }

    pub fn set_deadend(&self) {
        self.0.store(Self::TRUE, Ordering::SeqCst);
    }

    pub fn set_no_deadend(&self) {
        self.0.store(Self::FALSE, Ordering::SeqCst);
    }
}

/// Cincinnati configuration.
#[derive(Debug, Serialize, Clone)]
pub struct Cincinnati {
    /// Service base URL.
    pub base_url: String,
}

impl Cincinnati {
    /// Process Cincinnati configuration.
    #[context("failed to validate cincinnati configuration")]
    pub(crate) fn with_config(cfg: inputs::CincinnatiInput, id: &Identity) -> Result<Self> {
        if cfg.base_url.is_empty() {
            anyhow::bail!("empty Cincinnati base URL");
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
        denylisted_depls: BTreeSet<Release>,
        allow_downgrade: bool,
    ) -> Pin<Box<dyn Future<Output = Option<Release>>>> {
        UPDATE_CHECKS.inc();
        log::trace!("checking upstream Cincinnati server for updates");

        let update = self
            .next_update(id, denylisted_depls, allow_downgrade)
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
        denylisted_depls: BTreeSet<Release>,
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
                find_update(graph, booted, denylisted_depls, allow_downgrade)
            });
        Box::pin(next)
    }
}

/// Evaluate and record whether booted OS is a dead-end release, and
/// log that information in a MOTD file.
fn refresh_deadend_status(node: &Node) -> Result<()> {
    match evaluate_deadend(node) {
        Some(reason) => {
            BOOTED_DEADEND.set(1);
            if !DEADEND_STATE.is_deadend() {
                log::warn!("current release detected as dead-end, reason: {}", reason);
                std::process::Command::new("pkexec")
                    .arg("/usr/libexec/zincati")
                    .arg("deadend-motd")
                    .arg("set")
                    .arg("--reason")
                    .arg(reason)
                    .output()
                    .context("failed to write dead-end release information")?;
                DEADEND_STATE.set_deadend();
                log::debug!("MOTD updated with dead-end state");
            }
        }
        None => {
            BOOTED_DEADEND.set(0);
            if !DEADEND_STATE.is_no_deadend() {
                log::info!("current release detected as not a dead-end");
                std::process::Command::new("pkexec")
                    .arg("/usr/libexec/zincati")
                    .arg("deadend-motd")
                    .arg("unset")
                    .output()
                    .context("failed to remove dead-end release MOTD file")?;
                DEADEND_STATE.set_no_deadend();
                log::debug!("MOTD updated with no dead-end state");
            }
        }
    };
    Ok(())
}

/// Walk the graph, looking for an update reachable from the given digest.
fn find_update(
    graph: client::Graph,
    booted_depl: Release,
    denylisted_depls: BTreeSet<Release>,
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
        .find(|(_, node)| is_same_checksum(node, &booted_depl))
    {
        Some(current) => current,
        None => return Ok(None),
    };
    drop(booted_depl);
    let cur_release = Release::from_cincinnati(cur_node.clone())
        .map_err(|e| CincinnatiError::FailedNodeParsing(e.to_string()))?;

    if let Err(e) = refresh_deadend_status(cur_node) {
        log::warn!("failed to refresh dead-end status: {}", e);
    }
    // Evaluate and record whether booted OS is a dead-end release.
    // TODO(lucab): consider exposing this information in more places
    // (e.g. logs, motd, env/json file in a well-known location).
    let is_deadend: i64 = evaluate_deadend(cur_node).is_some().into();
    BOOTED_DEADEND.set(is_deadend);

    // Try to find all denylisted deployments in the graph too.
    let denylisted_releases = find_denylisted_releases(&graph, denylisted_depls);

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

    // Exclude targets in denylist.
    let new_updates = updates.difference(&denylisted_releases);

    // Log that we will avoid updating to denylisted releases.
    let prev_deployed_excluded = updates.intersection(&denylisted_releases).count();
    if prev_deployed_excluded > 0 {
        log::debug!(
            "Found {} possible update target{} present in denylist; ignoring",
            prev_deployed_excluded,
            if prev_deployed_excluded > 1 { "s" } else { "" }
        );
    }
    UPDATE_TARGETS_IGNORED.set(prev_deployed_excluded as i64);

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

/// Try to match a set of (denylisted) deployments to their graph entries.
fn find_denylisted_releases(graph: &client::Graph, depls: BTreeSet<Release>) -> BTreeSet<Release> {
    use std::collections::HashSet;

    let mut local_releases = BTreeSet::new();
    let payloads: HashSet<Payload> = depls
        .into_iter()
        // in the OCI case, the local deployment payload is a full OSTree image reference
        // while the cincinnati payload only contains the OCI pullspec
        // Extract the local image reference before comparing
        .map(|rel| match rel.payload {
            Payload::Checksum(_) => rel.payload,
            Payload::Pullspec(imgref) => {
                let ostree_imgref: Result<OstreeImageReference> = imgref.as_str().try_into();
                // ostree_imgref here is `ostree-remote-image:fedora:docker://quay.io.....`
                // while the cincinnati payload only contains `quay.io:///....`
                Payload::Pullspec(
                    ostree_imgref.map_or(imgref, |ostree_imgref| ostree_imgref.imgref.name),
                )
            }
        })
        .collect();

    for entry in &graph.nodes {
        if let Ok(release) = Release::from_cincinnati(entry.clone()) {
            if payloads.contains(&release.payload) {
                local_releases.insert(release);
            }
        }
    }

    local_releases
}

/// Check whether input node matches current checksum.
fn is_same_checksum(node: &Node, deploy: &Release) -> bool {
    match node.metadata.get(SCHEME_KEY) {
        Some(scheme) if scheme == OCI_SCHEME => {
            if let Ok(Some(local_digest)) = deploy.get_image_reference() {
                local_digest == node.payload
            } else {
                false
            }
        }
        Some(scheme) if scheme == CHECKSUM_SCHEME => {
            if let Payload::Checksum(checksum) = &deploy.payload {
                checksum == &node.payload
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Check whether input node is a dead-end; if so, return the reason.
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
        let current = Release {
            version: String::new(),
            payload: Payload::Checksum("current-sha".to_string()),
            age_index: None,
        };

        let mut metadata = HashMap::new();
        metadata.insert(SCHEME_KEY.to_string(), CHECKSUM_SCHEME.to_string());
        let matching = Node {
            version: "v0".to_string(),
            payload: "current-sha".to_string(),
            metadata,
        };
        assert!(is_same_checksum(&matching, &current));

        let mismatch = Node {
            version: "v0".to_string(),
            payload: "mismatch".to_string(),
            metadata: HashMap::new(),
        };
        assert!(!is_same_checksum(&mismatch, &current));
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
