mod cli_deploy;
mod cli_finalize;
mod cli_status;
pub use cli_status::booted;

mod actor;
pub use actor::{FinalizeDeployment, RpmOstreeClient, StageDeployment};

use crate::cincinnati::Node;
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
    pub fn from_cincinnati(node: Node) -> Self {
        Self {
            version: node.version,
            checksum: node.payload,
        }
    }
}
