//! TOML configuration fragments.

use serde::Deserialize;

/// Top-level configuration stanza.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct ConfigFragment {
    /// Cincinnati client configuration.
    pub(crate) cincinnati: Option<CincinnatiFragment>,
    /// Agent identity.
    pub(crate) identity: Option<IdentityFragment>,
    /// Update strategy configuration.
    pub(crate) updates: Option<UpdateFragment>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct IdentityFragment {
    /// Update group for this agent (default: 'default')
    pub(crate) group: Option<String>,
    /// Update group for this agent (default: derived from machine-id)
    pub(crate) node_uuid: Option<String>,
    /// Throttle bucket for this agent (default: dynamically computed)
    pub(crate) throttle_permille: Option<String>,
}

/// Config fragment for Cincinnati client.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct CincinnatiFragment {
    /// Base URL to upstream cincinnati server.
    pub(crate) base_url: Option<String>,
}

/// Config fragment for update logic.
#[derive(Debug, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct UpImmediateFragment {
    /// Whether to check for and fetch updates.
    pub(crate) fetch_updates: Option<String>,
    /// Whether to finalize staged updates.
    pub(crate) finalize_updates: Option<String>,
}

/// Config fragment for `remote_http` update strategy.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct UpHttpFragment {
    /// Base URL for the remote semaphore manager.
    pub(crate) base_url: Option<String>,
}

/// Config fragment for `periodic` update strategy.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct UpPeriodicFragment {
    // TODO(lucab): define entries.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn basic_dist_config_sample() {
        let fp = std::fs::File::open("dist/examples/00-config-sample.toml").unwrap();
        let mut bufrd = std::io::BufReader::new(fp);
        let mut content = vec![];
        bufrd.read_to_end(&mut content).unwrap();
        let cfg: ConfigFragment = toml::from_slice(&content).unwrap();

        let expected = ConfigFragment {
            cincinnati: Some(CincinnatiFragment {
                base_url: Some("http://example.com:80/".to_string()),
            }),
            identity: Some(IdentityFragment {
                group: Some("workers".to_string()),
                node_uuid: None,
                throttle_permille: None,
            }),
            updates: Some(UpdateFragment {
                strategy: Some("immediate".to_string()),
                immediate: Some(UpImmediateFragment {
                    fetch_updates: Some("true".to_string()),
                    finalize_updates: Some("true".to_string()),
                }),
                periodic: None,
                remote_http: None,
            }),
        };

        assert_eq!(cfg, expected);
    }
}
