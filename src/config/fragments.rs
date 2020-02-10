//! TOML configuration fragments.

use ordered_float::NotNan;
use serde::Deserialize;
use std::num::NonZeroU64;

/// Top-level configuration stanza.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct ConfigFragment {
    /// Agent configuration.
    pub(crate) agent: Option<AgentFragment>,
    /// Cincinnati client configuration.
    pub(crate) cincinnati: Option<CincinnatiFragment>,
    /// Agent identity.
    pub(crate) identity: Option<IdentityFragment>,
    /// Update strategy configuration.
    pub(crate) updates: Option<UpdateFragment>,
}

/// Config fragment for agent settings.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct AgentFragment {
    /// Timing settings for the agent.
    pub(crate) timing: Option<AgentTiming>,
}

/// Config fragment for agent timing.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct AgentTiming {
    /// Pausing interval between updates checks in steady mode, in seconds (default: 300).
    pub(crate) steady_interval_secs: Option<NonZeroU64>,
}

// Config fragment for agent identity.
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
    /// Whether to enable automatic downgrades.
    pub(crate) allow_downgrade: Option<bool>,
    /// Whether to enable auto-updates logic.
    pub(crate) enabled: Option<bool>,
    /// Update strategy (default: immediate).
    pub(crate) strategy: Option<String>,
    /// `fleet_lock` strategy config.
    pub(crate) fleet_lock: Option<UpdateFleetLock>,
}

/// Config fragment for `fleet_lock` update strategy.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct UpdateFleetLock {
    /// Base URL for the remote semaphore manager.
    pub(crate) base_url: Option<String>,
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
            agent: Some(AgentFragment {
                timing: Some(AgentTiming {
                    steady_interval_secs: Some(NonZeroU64::new(35).unwrap()),
                }),
            }),
            cincinnati: Some(CincinnatiFragment {
                base_url: Some("http://cincinnati.example.com:80/".to_string()),
            }),
            identity: Some(IdentityFragment {
                group: Some("workers".to_string()),
                node_uuid: Some("27e3ac02af3946af995c9940e18b0cce".to_string()),
                rollout_wariness: Some(NotNan::new(0.5).unwrap()),
            }),
            updates: Some(UpdateFragment {
                allow_downgrade: Some(true),
                enabled: Some(false),
                strategy: Some("fleet_lock".to_string()),
                fleet_lock: Some(UpdateFleetLock {
                    base_url: Some("http://fleet-lock.example.com:8080/".to_string()),
                }),
            }),
        };

        assert_eq!(cfg, expected);
    }
}
