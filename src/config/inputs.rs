use crate::config::fragments;
use failure::{Fallible, ResultExt};
use log::{debug, trace};
use serde::Serialize;

/// Runtime configuration holding environmental inputs.
#[derive(Debug, Serialize)]
pub(crate) struct ConfigInput {
    pub(crate) cincinnati: CincinnatiInput,
    pub(crate) updates: UpdateInput,
    pub(crate) identity: IdentityInput,
}

impl ConfigInput {
    /// Read config fragments and merge them into a single config.
    pub(crate) fn read_configs(dirs: &[&str], app_name: &str) -> Fallible<Self> {
        use std::io::Read;

        let mut fragments = Vec::new();
        for prefix in dirs {
            let dir = format!("{}/{}/config.d", prefix, app_name);
            debug!("scanning configuration directory '{}'", dir);

            let wildcard = format!("{}/*.toml", dir);
            let toml_files = glob::glob(&wildcard)?;
            for fpath in toml_files.filter_map(Result::ok) {
                trace!("reading config fragment '{}'", fpath.display());

                let fp = std::fs::File::open(&fpath)
                    .context(format!("failed to open file '{}'", fpath.display()))?;
                let mut bufrd = std::io::BufReader::new(fp);
                let mut content = vec![];
                bufrd
                    .read_to_end(&mut content)
                    .context(format!("failed to read content of '{}'", fpath.display()))?;
                let frag: fragments::ConfigFragment =
                    toml::from_slice(&content).context("failed to parse TOML")?;

                fragments.push(frag);
            }
        }

        let cfg = Self::merge_fragments(fragments);
        Ok(cfg)
    }

    /// Merge multiple fragments into a single configuration.
    fn merge_fragments(fragments: Vec<fragments::ConfigFragment>) -> Self {
        let mut cincinnatis = vec![];
        let mut updates = vec![];
        let mut identities = vec![];

        for snip in fragments {
            if let Some(c) = snip.cincinnati {
                cincinnatis.push(c);
            }
            if let Some(f) = snip.updates {
                updates.push(f);
            }
            if let Some(i) = snip.identity {
                identities.push(i);
            }
        }

        Self {
            cincinnati: CincinnatiInput::from_fragments(cincinnatis),
            updates: UpdateInput::from_fragments(updates),
            identity: IdentityInput::from_fragments(identities),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CincinnatiInput {
    /// Base URL (template) for the Cincinnati service.
    pub(crate) base_url: String,
}

impl CincinnatiInput {
    fn from_fragments(fragments: Vec<fragments::CincinnatiFragment>) -> Self {
        let mut cfg = Self {
            base_url: String::new(),
        };

        for snip in fragments {
            if let Some(u) = snip.base_url {
                cfg.base_url = u;
            }
        }

        cfg
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct IdentityInput {
    pub(crate) group: String,
    pub(crate) node_uuid: String,
    pub(crate) throttle_permille: Option<u16>,
}

impl IdentityInput {
    fn from_fragments(fragments: Vec<fragments::IdentityFragment>) -> Self {
        let mut cfg = Self {
            group: String::new(),
            node_uuid: String::new(),
            throttle_permille: None,
        };

        for snip in fragments {
            if let Some(g) = snip.group {
                cfg.group = g;
            }
            if let Some(nu) = snip.node_uuid {
                cfg.node_uuid = nu;
            }
            if let Some(tp) = snip.throttle_permille {
                cfg.throttle_permille = Some(tp.parse().unwrap());
            }
        }

        cfg
    }
}

/// Config for update logic.
#[derive(Debug, Serialize)]
pub(crate) struct UpdateInput {
    pub(crate) strategy: String,
    /// `remote_http` strategy config.
    pub(crate) remote_http: StratHttpInput,
    /// `periodic` strategy config.
    pub(crate) periodic: StratPeriodicInput,
}

impl UpdateInput {
    fn from_fragments(fragments: Vec<fragments::UpdateFragment>) -> Self {
        let mut strategy = String::new();
        let mut remote_http = StratHttpInput {
            base_url: String::new(),
        };
        let periodic = StratPeriodicInput {};

        for snip in fragments {
            if let Some(s) = snip.strategy {
                strategy = s;
            }
            if let Some(remote) = snip.remote_http {
                if let Some(b) = remote.base_url {
                    remote_http.base_url = b;
                }
            }
        }

        Self {
            strategy,
            remote_http,
            periodic,
        }
    }
}

/// Config snippet for `remote_http` finalizer strategy.
#[derive(Debug, Serialize)]
pub(crate) struct StratHttpInput {
    /// Base URL (template) for the remote semaphore manager.
    pub(crate) base_url: String,
}

/// Config snippet for `periodic` strategy.
#[derive(Debug, Serialize)]
pub(crate) struct StratPeriodicInput {}
