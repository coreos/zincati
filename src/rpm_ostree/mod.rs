mod cli_deploy;
mod cli_finalize;
mod cli_status;
pub use cli_status::{invoke_cli_status, parse_booted, parse_booted_updates_stream, FatalError};

mod actor;
pub use actor::{
    CleanupPendingDeployment, FinalizeDeployment, QueryLocalDeployments,
    QueryPendingDeploymentStream, RegisterAsDriver, RpmOstreeClient, StageDeployment,
};

#[cfg(test)]
mod mock_tests;

use crate::cincinnati::{Node, AGE_INDEX_KEY, CHECKSUM_SCHEME, SCHEME_KEY};
use anyhow::{anyhow, ensure, Context, Result};
use serde::Serialize;
use std::cmp::Ordering;

/// An OS release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Release {
    /// OS version.
    pub version: String,
    /// Image base checksum.
    pub checksum: String,
    /// Release age (Cincinnati `age_index`).
    pub age_index: Option<u64>,
}

impl std::cmp::Ord for Release {
    fn cmp(&self, other: &Self) -> Ordering {
        // Order is primarily based on age-index coming from Cincinnati.
        let self_age = self.age_index.unwrap_or(0);
        let other_age = other.age_index.unwrap_or(0);
        if self_age != other_age {
            return self_age.cmp(&other_age);
        }

        // As a fallback in case of duplicate age-index values, this tries
        // to disambiguate by picking an arbitrary lexicographic order.
        if self.version != other.version {
            return self.version.cmp(&other.version);
        }

        if self.checksum != other.checksum {
            return self.checksum.cmp(&other.checksum);
        }

        Ordering::Equal
    }
}

impl std::cmp::PartialOrd for Release {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Release {
    /// Builds a `Release` object from a Cincinnati node.
    pub fn from_cincinnati(node: Node) -> Result<Self> {
        ensure!(!node.version.is_empty(), "empty version field");
        ensure!(!node.payload.is_empty(), "empty payload field (checksum)");
        let scheme = node
            .metadata
            .get(SCHEME_KEY)
            .ok_or_else(|| anyhow!("missing metadata key: {}", SCHEME_KEY))?;

        ensure!(
            scheme == CHECKSUM_SCHEME,
            "unexpected payload scheme: {}",
            scheme
        );

        let age = {
            let val = node
                .metadata
                .get(AGE_INDEX_KEY)
                .ok_or_else(|| anyhow!("missing metadata key: {}", AGE_INDEX_KEY))?;

            val.parse::<u64>()
                .context(format!("invalid age_index value: {}", val))?
        };

        let rel = Self {
            version: node.version,
            checksum: node.payload,
            age_index: Some(age),
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
                AGE_INDEX_KEY.to_string() => "0".to_string(),
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
            metadata: hashmap! {
                SCHEME_KEY.to_string() => CHECKSUM_SCHEME.to_string(),
            },
        };
        Release::from_cincinnati(node3).unwrap_err();

        let node4 = Node {
            version: "mock-version".to_string(),
            payload: "mock-payload".to_string(),
            metadata: hashmap! {},
        };
        Release::from_cincinnati(node4).unwrap_err();
    }

    #[test]
    fn release_cmp() {
        {
            let n0 = Release {
                version: "v0".to_string(),
                checksum: "p0".to_string(),
                age_index: Some(0),
            };
            let n1 = Release {
                version: "v1".to_string(),
                checksum: "p1".to_string(),
                age_index: Some(1),
            };
            assert_eq!(n0 < n1, true);
            assert_eq!(n0 == n0, true);
            assert_eq!(n0 < n0, false);
            assert_eq!(n0 > n0, false);
        }
        {
            let n0 = Release {
                version: "v0".to_string(),
                checksum: "p0".to_string(),
                age_index: Some(0),
            };
            let n1 = Release {
                version: "v1".to_string(),
                checksum: "p1".to_string(),
                age_index: Some(0),
            };
            assert_eq!(n0 < n1, true);
            assert_eq!(n0 < n0, false);
            assert_eq!(n0 > n0, false);
        }
        {
            let n0 = Release {
                version: "v0".to_string(),
                checksum: "p0".to_string(),
                age_index: Some(0),
            };
            let n1 = Release {
                version: "v0".to_string(),
                checksum: "p1".to_string(),
                age_index: Some(0),
            };
            assert_eq!(n0 < n1, true);
            assert_eq!(n0 < n0, false);
            assert_eq!(n0 > n0, false);
        }
    }
}
