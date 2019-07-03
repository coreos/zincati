mod cli_deploy;
mod cli_finalize;
mod cli_status;
pub use cli_status::{basearch, booted};

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
