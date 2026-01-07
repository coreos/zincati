mod cli_deploy;
mod cli_finalize;
mod cli_status;
pub use cli_status::{
    invoke_cli_status, parse_booted, parse_booted_updates_stream, SystemInoperable,
};

mod actor;
pub use actor::{
    CleanupPendingDeployment, FinalizeDeployment, QueryLocalDeployments,
    QueryPendingDeploymentStream, RegisterAsDriver, RpmOstreeClient, StageDeployment,
};
use ostree_ext::oci_spec::distribution::Reference;

#[cfg(test)]
mod mock_tests;

use crate::cincinnati::{Node, AGE_INDEX_KEY, OCI_SCHEME, SCHEME_KEY};
use anyhow::{anyhow, bail, ensure, Context, Result};
use core::fmt;
use serde::Serialize;
use std::cmp::Ordering;

/// An OS release, as described by the cincinnati graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Release {
    /// OS version.
    pub version: String,
    /// Image base checksum or OCI pullspec.
    pub payload: Payload,
    /// Release age (Cincinnati `age_index`).
    pub age_index: Option<u64>,
}

/// payload unique identifier can either be an ostree checksum or an OCI pullspec
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize)]
pub enum Payload {
    /// an OCI image reference
    Pullspec(Reference),
}

impl std::fmt::Display for Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Payload::Pullspec(image) => write!(f, "{}", image.whole()),
        }
    }
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

        if self.payload != other.payload {
            let self_payload = self.payload.to_string();
            return self_payload.cmp(&other.payload.to_string());
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

        let payload = match scheme.as_str() {
            OCI_SCHEME => Payload::Pullspec(node.payload.parse()?),
            _ => bail!("unexpected payload scheme: {}", scheme),
        };

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
            payload,
            age_index: Some(age),
        };
        Ok(rel)
    }
    pub fn get_image_reference(&self) -> Result<Option<String>> {
        match &self.payload {
            Payload::Pullspec(imgref) => Ok(Some(imgref.whole())),
        }
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
    #[allow(clippy::nonminimal_bool)]
    fn release_cmp() {
        {
            let n0 = Release {
                version: "v0".to_string(),
                payload: Payload::Checksum("p0".to_string()),
                age_index: Some(0),
            };
            let n1 = Release {
                version: "v1".to_string(),
                payload: Payload::Checksum("p1".to_string()),
                age_index: Some(1),
            };
            assert!(n0 < n1);
            assert!(n0 == n0);
            assert!(!(n0 < n0));
            assert!(!(n0 > n0));
        }
        {
            let n0 = Release {
                version: "v0".to_string(),
                payload: Payload::Checksum("p0".to_string()),
                age_index: Some(0),
            };
            let n1 = Release {
                version: "v1".to_string(),
                payload: Payload::Checksum("p1".to_string()),
                age_index: Some(0),
            };
            assert!(n0 < n1);
            assert!(!(n0 < n0));
            assert!(!(n0 > n0));
        }
        {
            let n0 = Release {
                version: "v0".to_string(),
                payload: Payload::Checksum("p0".to_string()),
                age_index: Some(0),
            };
            let n1 = Release {
                version: "v0".to_string(),
                payload: Payload::Checksum("p1".to_string()),
                age_index: Some(0),
            };
            assert!(n0 < n1);
            assert!(!(n0 < n0));
            assert!(!(n0 > n0));
        }
    }
}
