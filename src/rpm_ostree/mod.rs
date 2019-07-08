mod cli_deploy;
mod cli_finalize;
mod cli_status;
pub use cli_status::{basearch, booted, updates_stream};

mod actor;
pub use actor::{FinalizeDeployment, RpmOstreeClient, StageDeployment};

use crate::cincinnati::{Node, CHECKSUM_SCHEME, SCHEME_KEY};
use failure::{ensure, format_err, Fallible};
use serde::Serialize;

/// An OS release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Release {
    /// OS version.
    pub version: String,
    /// Image base checksum.
    pub checksum: String,
}

impl Release {
    /// Builds a `Release` object from a Cincinnati node.
    pub fn from_cincinnati(node: Node) -> Fallible<Self> {
        ensure!(!node.version.is_empty(), "empty version field");
        ensure!(!node.payload.is_empty(), "empty payload field (checksum)");
        let scheme = node
            .metadata
            .get(SCHEME_KEY)
            .ok_or_else(|| format_err!("missing metadata key: {}", SCHEME_KEY))?;

        ensure!(
            scheme == CHECKSUM_SCHEME,
            "unexpected payload scheme: {}",
            scheme
        );

        let rel = Self {
            version: node.version,
            checksum: node.payload,
        };
        Ok(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashmap;

    #[test]
    fn release_from_cincinnati() {
        let input = Node {
            version: "mock-version".to_string(),
            payload: "mock-payload".to_string(),
            metadata: hashmap! {
                SCHEME_KEY.to_string() => CHECKSUM_SCHEME.to_string(),
            },
        };
        Release::from_cincinnati(input).unwrap();
    }

    #[test]
    fn invalid_node() {
        let node1 = Node {
            version: "".to_string(),
            payload: "mock-payload".to_string(),
            metadata: hashmap! {
                SCHEME_KEY.to_string() => CHECKSUM_SCHEME.to_string(),
            },
        };
        Release::from_cincinnati(node1).unwrap_err();

        let node2 = Node {
            version: "mock-version".to_string(),
            payload: "".to_string(),
            metadata: hashmap! {
                SCHEME_KEY.to_string() => CHECKSUM_SCHEME.to_string(),
            },
        };
        Release::from_cincinnati(node2).unwrap_err();

        let node3 = Node {
            version: "mock-version".to_string(),
            payload: "mock-payload".to_string(),
            metadata: hashmap! {},
        };
        Release::from_cincinnati(node3).unwrap_err();
    }
}
