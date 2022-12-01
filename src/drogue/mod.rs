use crate::{
    config::inputs::DrogueInput,
    drogue::mqtt::MqttClient,
    identity::Identity,
    rpm_ostree::{DeploymentJson, GetFullState, RpmOstreeClient, StatusJson},
    update_agent::{StartUpgrade, SubscribeState, UpdateAgent},
};
use actix::Addr;
use anyhow::{bail, Context};
use log::{debug, info, trace, warn};
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, Outgoing, Publish, QoS};
use serde::Serialize;
use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    str::from_utf8,
    sync::Arc,
    time::Duration,
};
use tokio::{
    select,
    sync::{broadcast::error::RecvError, oneshot, RwLock},
    time::MissedTickBehavior,
};

mod mqtt;

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Config {
    pub enabled: bool,

    pub mqtt: MqttClient,

    pub application: String,
    pub device: String,
    // FIXME: need to add TLS-PSK and X.509 here too
    pub password: String,

    pub event_buffer: usize,
}

impl Config {
    pub(crate) fn with_config(input: DrogueInput, identity: &Identity) -> anyhow::Result<Self> {
        if input.mqtt.hostname.is_empty() {
            bail!("Empty MQTT hostname");
        }

        Ok(Config {
            enabled: input.enabled,
            mqtt: MqttClient {
                host: input.mqtt.hostname,
                port: input.mqtt.port.into(),
                disable_tls: input.mqtt.disable_tls,
                insecure: input.mqtt.insecure,
                initial_reconnect_delay: input.mqtt.initial_reconnect_delay,
                keepalive: input.mqtt.keepalive,
                client_id: input.mqtt.client_id,
            },
            application: input.application,
            device: input
                .device
                .unwrap_or_else(|| identity.node_uuid.dashed_hex()),
            password: input.password,
            event_buffer: 128,
        })
    }
}

#[derive(Clone)]
pub struct Agent {
    _client: AsyncClient,
    _inner: Arc<Inner>,
}

struct Inner {
    _shutdown: Option<oneshot::Sender<()>>,
}

struct Runner {
    state: Arc<RwLock<State>>,
    client: AsyncClient,
    update: Addr<UpdateAgent>,
    ostree: Addr<RpmOstreeClient>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    state: HashMap<String, Vec<u8>>,
}

impl State {
    pub fn set<C, P>(&mut self, channel: C, payload: &P) -> anyhow::Result<Update>
    where
        C: Into<String>,
        P: Serialize + Debug,
    {
        let channel = channel.into();
        trace!("Setting new state: {channel} = {payload:?}");
        let payload = serde_json::to_vec(payload)?;

        let update = match self.state.entry(channel.clone()) {
            Entry::Occupied(mut entry) => {
                let current = entry.get_mut();
                if current != &payload {
                    *current = payload.clone();
                    vec![(channel, payload)]
                } else {
                    vec![]
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(payload.clone());
                vec![(channel, payload)]
            }
        };

        Ok(Update(update))
    }

    pub fn get(&self) -> Update {
        Update(
            self.state
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    }
}

/// Updates to publish
pub struct Update(Vec<(String, Vec<u8>)>);

impl Update {
    /// Publish changes to an MQTT client.
    pub fn publish(self, client: &AsyncClient) -> anyhow::Result<()> {
        for p in self.0 {
            client.try_publish(p.0, QoS::AtMostOnce, false, p.1)?;
        }
        Ok(())
    }
}

impl Agent {
    pub(crate) fn start(
        config: Config,
        update: Addr<UpdateAgent>,
        ostree: Addr<RpmOstreeClient>,
    ) -> anyhow::Result<Self> {
        let mut options: MqttOptions = config.mqtt.try_into()?;

        let device: String =
            url::form_urlencoded::byte_serialize(config.device.as_bytes()).collect();
        options.set_credentials(
            format!("{}@{}", device, config.application),
            config.password,
        );

        let (client, event_loop) = AsyncClient::new(options, config.event_buffer);
        let (tx, rx) = oneshot::channel();

        let runner = Runner {
            state: Default::default(),
            client: client.clone(),
            update,
            ostree,
        };
        tokio::spawn(async move {
            let _ = runner.run(rx, event_loop).await;
        });

        Ok(Self {
            _client: client,
            _inner: Arc::new(Inner {
                _shutdown: Some(tx),
            }),
        })
    }
}

impl Runner {
    async fn run(self, rx: oneshot::Receiver<()>, event_loop: EventLoop) -> anyhow::Result<()> {
        info!("Running event loop...");

        select! {
            _ = self.run_loop(event_loop) => {},
            _ = self.run_updater() => {},
            _ = self.run_ostree() => {},
            _ = rx => {},
        }

        info!("Exiting runner...");

        Ok(())
    }

    /// Run updating the state from the OStree client
    async fn run_ostree(&self) -> anyhow::Result<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            match self.ostree.send(GetFullState).await {
                Ok(Ok(result)) => {
                    debug!("Current OStree state: {result:?}");
                    self.state
                        .write()
                        .await
                        .set("ostree", &OsTreeState::from(result))?
                        .publish(&self.client)?;
                }
                other => {
                    info!("Unexpected outcome of refresh: {other:?}");
                }
            }

            interval.tick().await;
        }
    }

    /// Run updating the state information.
    async fn run_updater(&self) -> anyhow::Result<()> {
        let mut rx = self.update.send(SubscribeState).await?;

        loop {
            match rx.recv().await {
                Ok(state) => {
                    self.state
                        .write()
                        .await
                        .set("updater", &state)?
                        .publish(&self.client)?;
                }
                Err(RecvError::Lagged(amount)) => {
                    info!("Lagged {amount} updates");
                }
                Err(RecvError::Closed) => return Ok(()),
            }
        }
    }

    /// Drive the MQTT event loop.
    ///
    /// This will also handle re-connects.
    async fn run_loop(&self, mut event_loop: EventLoop) {
        loop {
            match event_loop.poll().await {
                Err(err) => {
                    info!("Connection error: {err}");
                    // keep going, as it will re-connect
                }
                Ok(Event::Incoming(Incoming::ConnAck(ack))) => {
                    info!("Connection opened: {ack:?}");
                    if let Err(err) = self.handle_connected().await {
                        warn!("Failed to handle connection: {err}");
                        if let Err(err) = self.client.disconnect().await {
                            panic!("Failed to disconnect after error: {err}");
                        }
                    }
                }
                Ok(Event::Incoming(Incoming::SubAck(ack))) => {
                    debug!("SubAck: {ack:?}");
                }
                Ok(Event::Incoming(Incoming::PubAck(ack))) => {
                    debug!("PubAck: {ack:?}");
                }
                Ok(Event::Incoming(Incoming::Publish(publish))) => {
                    if let Err(err) = self.handle_msg(publish).await {
                        warn!("Failed to handle command: {err}");
                        if let Err(err) = self.client.disconnect().await {
                            panic!("Failed to disconnect after error: {err}");
                        }
                    }
                }
                Ok(
                    Event::Incoming(Incoming::PingResp)
                    | Event::Outgoing(
                        Outgoing::PingReq | Outgoing::Subscribe(_) | Outgoing::Publish(_),
                    ),
                ) => {
                    // ignore
                }
                Ok(event) => {
                    info!("Unexpected event: {event:?}");
                }
            }
        }
    }

    /// Handle incoming MQTT messages.
    async fn handle_msg(&self, publish: Publish) -> anyhow::Result<()> {
        let topic = &publish.topic;
        debug!("Command: {topic}");

        match topic.split('/').collect::<Vec<_>>().as_slice() {
            ["command", "inbox", "", command] => {
                if let Err(err) = self.handle_cmd(command, &publish.payload).await {
                    // there isn't much we can do, log and move on
                    warn!("Failed to process command: {err}");
                }
            }
            _ => {
                bail!("Invalid command: {topic}");
            }
        }

        self.client.ack(&publish).await?;

        Ok(())
    }

    async fn handle_cmd(&self, command: &str, payload: &[u8]) -> anyhow::Result<()> {
        match command {
            "update" => {
                debug!("Update request - payload: {:?}", from_utf8(payload));
                let release = serde_json::from_slice(payload)?;
                self.update
                    .try_send(StartUpgrade(release))
                    .context("Triggering update")?;
            }
            command => {
                bail!("Unknown command: {command}");
            }
        }

        Ok(())
    }

    /// Handle the case the connection was established.
    async fn handle_connected(&self) -> anyhow::Result<()> {
        self.client
            .subscribe("command/inbox//#", QoS::AtLeastOnce)
            .await?;

        // send last known state
        self.state.read().await.get().publish(&self.client)?;

        // ok
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentState {
    pub version: String,
    pub booted: bool,
    pub staged: bool,
    pub checksum: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_image_reference: Option<String>,
    pub base_metadata: BaseMetadataState,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseMetadataState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
}

impl From<DeploymentJson> for DeploymentState {
    fn from(json: DeploymentJson) -> Self {
        Self {
            version: json.version,
            booted: json.booted,
            staged: json.staged,
            checksum: json.checksum,
            base_checksum: json.base_checksum,
            container_image_reference: json.container_image_reference,
            base_metadata: BaseMetadataState {
                stream: json.base_metadata.stream,
            },
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsTreeState {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deployments: Vec<DeploymentState>,
}

impl From<StatusJson> for OsTreeState {
    fn from(json: StatusJson) -> Self {
        Self {
            deployments: json.deployments.into_iter().map(|s| s.into()).collect(),
        }
    }
}
