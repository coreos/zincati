mod cli_status;
mod cli_upgrade;

mod actor;
pub use actor::{RpmOstreeClient, StageDeployment};

use crate::cincinnati::Node;

/// An OS release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// OS version.
    pub version: String,
    /// Image checksum
    pub checksum: String,
}

impl Release {
    /// Builds a `Release` object from a Cincinnati node.
    pub fn from_cincinnati(node: Node) -> Self {
        Self {
            version: node.version,
            checksum: node.payload,
        }
    }

    /// Returns the reference ID for this release.
    pub fn reference_id(&self) -> String {
        format!("revision={}", self.checksum)
    }
}
