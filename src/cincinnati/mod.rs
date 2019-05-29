//! Asynchronous Cincinnati client.

// Cincinnati client.
mod client;
pub use client::Node;

#[cfg(test)]
mod mock_tests;

use crate::config::inputs;
use crate::identity::Identity;
use failure::{bail, format_err, Error, Fallible};
use futures::future;
use futures::prelude::*;
use serde::Serialize;

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
            .map_err(|e| log::error!("failed to check for updates: {}", e));
        Box::new(update)
    }

    /// Get the next update.
    fn next_update(&self, id: &Identity) -> Box<Future<Item = Option<Node>, Error = Error>> {
        let params = id.cincinnati_params();
        let cur_version = id.current_version.clone();
        let client = client::ClientBuilder::new(self.base_url.to_string())
            .query_params(Some(params))
            .build();

        let next = future::result(client)
            .and_then(|c| c.fetch_graph())
            .and_then(|graph| find_update(graph, cur_version))
            .map_err(|e| format_err!("failed to query Cincinnati: {}", e));
        Box::new(next)
    }
}

/// Walk the graph, looking for an update reachable from current version.
fn find_update(graph: client::Graph, cur_version: String) -> Fallible<Option<Node>> {
    let cur_position = match graph.nodes.iter().position(|n| n.version == cur_version) {
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
