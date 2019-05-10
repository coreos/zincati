use failure::{Fallible, ResultExt};
use serde::Serialize;
use uuid::Uuid;

/// Default group for reboot management.
static DEFAULT_GROUP: &str = "default";

/// Agent identity.
#[derive(Debug, Serialize)]
pub(crate) struct Identity {
    /// OS base architecture.
    pub(crate) basearch: String,
    /// Current OS version.
    pub(crate) current_version: String,
    /// Update groupd.
    pub(crate) group: String,
    /// Unique node identifier.
    pub(crate) node_uuid: Uuid,
    /// OS platform.
    pub(crate) platform: String,
    /// Stream label.
    pub(crate) stream: String,
    /// Optional throttle level, 0 (never) to 1000 (unlimited).
    pub(crate) throttle_permille: Option<u16>,
}

impl Identity {
    /*
    pub(crate) fn with_config(cfg: IdentityInput) -> Fallible<Self> {
        let mut id = Self::try_default().context("failed to build default identity")?;

        if !cfg.group.is_empty() {
            id.group = cfg.group;
        };

        if !cfg.node_uuid.is_empty() {
            id.node_uuid = Uuid::parse_str(&cfg.node_uuid).context("failed to parse uuid")?;
        }

        if let Some(tp) = cfg.throttle_permille {
            id.throttle_permille = Some(tp);
        }

        Ok(id)
    }
    */

    /// Try to build default agent identity.
    pub fn try_default() -> Fallible<Self> {
        // TODO(lucab): populate these.
        let basearch = read_basearch()?;
        let stream = read_stream()?;
        let platform = read_platform_id()?;
        let node_uuid = compute_node_uuid()?;
        let current_version = read_os_version().context("failed to get current OS version")?;

        let id = Self {
            basearch,
            stream,
            platform,
            current_version,
            group: DEFAULT_GROUP.to_string(),
            node_uuid,
            throttle_permille: None,
        };
        Ok(id)
    }
}

fn read_stream() -> Fallible<String> {
    // TODO(lucab): read this from os-release.
    let ver = "stable".to_string();
    Ok(ver)
}

fn read_platform_id() -> Fallible<String> {
    // TODO(lucab): read this from kernel command-line.
    let ver = "metal-bios".to_string();
    Ok(ver)
}

fn read_basearch() -> Fallible<String> {
    // TODO(lucab): read this from os-release.
    let ver = "amd64".to_string();
    Ok(ver)
}

fn read_os_version() -> Fallible<String> {
    // TODO(lucab): read this from os-release.
    let ver = "FCOS-01".to_string();
    Ok(ver)
}

fn compute_node_uuid() -> Fallible<Uuid> {
    // TODO(lucab): hash machine-id.
    let node_uuid = Uuid::from_u128(0);
    Ok(node_uuid)
}
