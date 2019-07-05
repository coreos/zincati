mod platform;

use crate::config::inputs;
use crate::rpm_ostree;
use failure::{format_err, Fallible, ResultExt};
use libsystemd::id128;
use serde::Serialize;
use std::collections::HashMap;

/// Default group for reboot management.
static DEFAULT_GROUP: &str = "default";

/// Application ID (`de35106b6ec24688b63afddaa156679b`)
static APP_ID: &[u8] = &[
    0xde, 0x35, 0x10, 0x6b, 0x6e, 0xc2, 0x46, 0x88, 0xb6, 0x3a, 0xfd, 0xda, 0xa1, 0x56, 0x67, 0x9b,
];

/// Agent identity.
#[derive(Debug, Serialize)]
pub(crate) struct Identity {
    /// OS base architecture.
    pub(crate) basearch: String,
    /// Current OS (version and deployment base-checksum).
    pub(crate) current_os: rpm_ostree::Release,
    /// Update groupd.
    pub(crate) group: String,
    /// Unique node identifier.
    pub(crate) node_uuid: id128::Id128,
    /// OS platform.
    pub(crate) platform: String,
    /// Stream label.
    pub(crate) stream: String,
    /// Optional throttle level, 0 (never) to 1000 (unlimited).
    pub(crate) throttle_permille: Option<u16>,
}

impl Identity {
    /// Create from configuration.
    pub(crate) fn with_config(cfg: inputs::IdentityInput) -> Fallible<Self> {
        let mut id = Self::try_default().context("failed to build default identity")?;

        if !cfg.group.is_empty() {
            id.group = cfg.group;
        };

        if !cfg.node_uuid.is_empty() {
            id.node_uuid = id128::Id128::parse_str(&cfg.node_uuid)
                .map_err(|e| format_err!("failed to parse node UUID: {}", e))?;
        }

        if let Some(tp) = cfg.throttle_permille {
            id.throttle_permille = Some(tp);
        }

        Ok(id)
    }

    /// Try to build default agent identity.
    pub fn try_default() -> Fallible<Self> {
        let basearch = rpm_ostree::basearch()?;
        let current_os = rpm_ostree::booted().context("failed to introspect booted OS image")?;
        let node_uuid = {
            let app_id = id128::Id128::try_from_slice(APP_ID)
                .map_err(|e| format_err!("failed to parse application ID: {}", e))?;
            compute_node_uuid(&app_id)?
        };
        let platform = platform::read_id("/proc/cmdline")?;
        // TODO(lucab): populate this from node introspection.
        let stream = read_stream()?;

        let id = Self {
            basearch,
            stream,
            platform,
            current_os,
            group: DEFAULT_GROUP.to_string(),
            node_uuid,
            throttle_permille: None,
        };
        Ok(id)
    }

    /// Return context variables for URL templates.
    pub fn url_variables(&self) -> HashMap<String, String> {
        // This explicitly does not include "current_version",
        // "throttle_permille" and "node_uuid".
        let mut vars = HashMap::new();
        vars.insert("basearch".to_string(), self.basearch.clone());
        vars.insert("group".to_string(), self.group.clone());
        vars.insert("platform".to_string(), self.platform.clone());
        vars.insert("stream".to_string(), self.stream.clone());
        vars
    }

    /// Return Cincinnati client parameters.
    pub fn cincinnati_params(&self) -> HashMap<String, String> {
        let mut vars = HashMap::new();
        vars.insert("basearch".to_string(), self.basearch.clone());
        vars.insert("current_os".to_string(), self.current_os.checksum.clone());
        vars.insert("group".to_string(), self.group.clone());
        vars.insert("node_uuid".to_string(), self.node_uuid.lower_hex());
        vars.insert("platform".to_string(), self.platform.clone());
        vars.insert("stream".to_string(), self.stream.clone());
        if let Some(val) = self.throttle_permille {
            vars.insert("throttle_permille".to_string(), val.to_string());
        }
        vars
    }

    #[cfg(test)]
    pub(crate) fn mock_default(throttle_permille: Option<u16>) -> Self {
        Self {
            basearch: "mock-amd64".to_string(),
            current_os: rpm_ostree::Release {
                version: "0.0.0-mock".to_string(),
                checksum: "sha-mock".to_string(),
            },
            group: "mock-workers".to_string(),
            node_uuid: id128::Id128::parse_str("e0f3745b108f471cbd4883c6fbed8cdd").unwrap(),
            platform: "mock-azure".to_string(),
            stream: "mock-stable".to_string(),
            throttle_permille,
        }
    }
}

fn read_stream() -> Fallible<String> {
    // TODO(lucab): read this from os-release.
    let ver = "testing".to_string();
    Ok(ver)
}

fn compute_node_uuid(app_id: &id128::Id128) -> Fallible<id128::Id128> {
    let id = id128::get_machine_app_specific(app_id)
        .map_err(|e| format_err!("failed to get node ID: {}", e))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_url_variables() {
        let id = Identity::mock_default(Some(500));
        let vars = id.url_variables();

        assert!(vars.contains_key("basearch"));
        assert!(vars.contains_key("group"));
        assert!(vars.contains_key("platform"));
        assert!(vars.contains_key("stream"));
        assert!(!vars.contains_key("node_uuid"));
        assert!(!vars.contains_key("current_os"));
        assert!(!vars.contains_key("throttle_permille"));
    }

    #[test]
    fn identity_cincinnati_params() {
        let id = Identity::mock_default(Some(500));
        let vars = id.cincinnati_params();

        assert!(vars.contains_key("basearch"));
        assert!(vars.contains_key("group"));
        assert!(vars.contains_key("platform"));
        assert!(vars.contains_key("stream"));
        assert!(vars.contains_key("node_uuid"));
        assert!(vars.contains_key("current_os"));

        let throttle = vars.get("throttle_permille").unwrap();
        assert_eq!(throttle, "500")
    }
}
