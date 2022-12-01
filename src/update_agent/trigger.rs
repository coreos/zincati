use crate::cincinnati::Cincinnati;
use crate::identity::Identity;
use crate::update_agent::UpdateAgentState;
use crate::utils;
use log::trace;

/// The way updates are controlled.
#[derive(Clone, Debug)]
pub enum Trigger {
    /// Triggered by polling cincinnati for new updates.
    Cincinnati(TriggerCincinnati),
    /// Triggered by a remote command (e.g. through the Drogue IoT MQTT endpoint).
    Remote,
}

impl Trigger {
    /// Create a new Cincinnati trigger.
    pub(crate) fn cincinnati(
        cincinnati: Cincinnati,
        identity: Identity,
        allow_downgrade: bool,
    ) -> Self {
        Self::Cincinnati(TriggerCincinnati {
            cincinnati,
            identity,
            allow_downgrade,
        })
    }
}

#[derive(Clone, Debug)]
pub struct TriggerCincinnati {
    /// Allowing downgrades.
    allow_downgrade: bool,
    /// Agent identity.
    identity: Identity,
    /// Cincinnati client.
    cincinnati: Cincinnati,
}

impl TriggerCincinnati {
    async fn tick(&self, state: &mut UpdateAgentState) {
        trace!("trying to check for updates (cincinatti)");

        let timestamp_now = chrono::Utc::now();
        utils::update_unit_status(&format!(
            "periodically polling for updates (last checked {})",
            timestamp_now.format("%a %Y-%m-%d %H:%M:%S %Z")
        ));
        let allow_downgrade = self.allow_downgrade;

        let release = self
            .cincinnati
            .fetch_update_hint(&self.identity, state.denylist.clone(), allow_downgrade)
            .await;

        match release {
            Some(release) => {
                utils::update_unit_status(&format!("found update on remote: {}", release.version));
                state.machine_state.update_available(release);
            }
            None => {
                state.machine_state.no_new_update();
            }
        }
    }
}

impl Trigger {
    pub async fn tick(&self, state: &mut UpdateAgentState) {
        match self {
            Self::Cincinnati(cincinnati) => cincinnati.tick(state).await,
            Self::Remote => state.machine_state.no_new_update(),
        }
    }
}
