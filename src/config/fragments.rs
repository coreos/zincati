//! TOML configuration fragments.

use ordered_float::NotNan;
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
    /// Update group for this agent (default: derived server-side)
    pub(crate) rollout_wariness: Option<NotNan<f64>>,
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
    /// Whether to enable auto-updates logic.
    pub(crate) enabled: Option<bool>,
    /// Update strategy (default: immediate).
    pub(crate) strategy: Option<String>,
    /// `periodic` strategy config.
    pub(crate) periodic: Option<UpPeriodicFragment>,
    /// `remote_http` strategy config.
    pub(crate) remote_http: Option<UpHttpFragment>,
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
        let fp = std::fs::File::open("tests/fixtures/00-config-sample.toml").unwrap();
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
                node_uuid: Some("27e3ac02af3946af995c9940e18b0cce".to_string()),
                rollout_wariness: Some(NotNan::new(0.5).unwrap()),
            }),
            updates: Some(UpdateFragment {
                enabled: Some(false),
                strategy: Some("immediate".to_string()),
                periodic: None,
                remote_http: None,
            }),
        };

        assert_eq!(cfg, expected);
    }
}
