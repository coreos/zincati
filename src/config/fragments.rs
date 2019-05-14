//! TOML configuration fragments.

use serde::Deserialize;

/// Top-level configuration stanza.
#[derive(Debug, Deserialize)]
pub(crate) struct ConfigFragment {
    /// Cincinnati client configuration.
    pub(crate) cincinnati: Option<CincinnatiFragment>,
    /// Agent identity.
    pub(crate) identity: Option<IdentityFragment>,
    /// Update strategy configuration.
    pub(crate) updates: Option<UpdateFragment>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IdentityFragment {
    /// Update group for this agent (default: 'default')
    pub(crate) group: Option<String>,
    /// Update group for this agent (default: derived from machine-id)
    pub(crate) node_uuid: Option<String>,
    /// Throttle bucket for this agent (default: dynamically computed)
    pub(crate) throttle_permille: Option<String>,
}

/// Config fragment for Cincinnati client.
#[derive(Debug, Deserialize)]
pub(crate) struct CincinnatiFragment {
    /// Base URL to upstream cincinnati server.
    pub(crate) base_url: Option<String>,
}

/// Config fragment for update logic.
#[derive(Debug, Deserialize)]
pub(crate) struct UpdateFragment {
    /// Update strategy (default: immediate)
    pub(crate) strategy: Option<String>,
    /// `immediate` strategy config.
    pub(crate) immediate: Option<UpImmediateFragment>,
    /// `periodic` strategy config.
    pub(crate) periodic: Option<UpPeriodicFragment>,
    /// `remote_http` strategy config.
    pub(crate) remote_http: Option<UpHttpFragment>,
}

/// Config fragment for `immediate` update strategy.
#[derive(Debug, Deserialize)]
pub(crate) struct UpImmediateFragment {
    /// Whether to check for and fetch updates.
    pub(crate) fetch_updates: Option<String>,
    /// Whether to finalize staged updates.
    pub(crate) finalize_updates: Option<String>,
}

/// Config fragment for `remote_http` update strategy.
#[derive(Debug, Deserialize)]
pub(crate) struct UpHttpFragment {
    /// Base URL for the remote semaphore manager.
    pub(crate) base_url: Option<String>,
}

/// Config fragment for `periodic` update strategy.
#[derive(Debug, Deserialize)]
pub(crate) struct UpPeriodicFragment {
    // TODO(lucab): define entries.
}
