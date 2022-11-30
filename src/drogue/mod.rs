use crate::config::inputs::DrogueInput;
use crate::drogue::mqtt::MqttClient;
use crate::identity::Identity;
use crate::update_agent::{AgentState, StartUpgrade, SubscribeState, UpdateAgent};
use actix::Addr;
use anyhow::{bail, Context};
use log::{debug, info, warn};
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, Outgoing, Publish, QoS};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::RwLock;
use tokio::{select, sync::oneshot};

mod mqtt;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub enabled: bool,

    pub mqtt: MqttClient,

    pub application: String,
    pub device: String,
    // FIXME: need to add TLS-PSK and X.509 here too
    pub password: String,

    #[serde(default = "default::inflight_messages")]
    pub inflight_messages: usize,
}

mod default {
    pub const fn inflight_messages() -> usize {
        10
    }
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
            },
            application: input.application,
            device: input
                .device
                .unwrap_or_else(|| identity.node_uuid.dashed_hex()),
            password: input.password,
            inflight_messages: 10,
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
    state: Arc<RwLock<Option<AgentState>>>,
    client: AsyncClient,
    update: Addr<UpdateAgent>,
}

impl Agent {
    pub(crate) fn start(config: Config, update: Addr<UpdateAgent>) -> anyhow::Result<Self> {
        let mut options: MqttOptions = config.mqtt.try_into()?;

        let device: String =
            url::form_urlencoded::byte_serialize(config.device.as_bytes()).collect();
        options.set_credentials(
            format!("{}@{}", device, config.application),
            config.password,
        );

        let (client, event_loop) = AsyncClient::new(options, config.inflight_messages);
        let (tx, rx) = oneshot::channel();

        let runner = Runner {
            state: Default::default(),
            client: client.clone(),
            update,
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
            _ = self.run_update() => {},
            _ = rx => {},
        }

        info!("Exiting runner...");

        Ok(())
    }

    async fn run_update(&self) -> anyhow::Result<()> {
        let mut rx = self.update.send(SubscribeState).await?;

        loop {
            match rx.recv().await {
                Ok(state) => {
                    let payload = serde_json::to_vec(&state)?;

                    *self.state.write().await = Some(state);

                    self.client
                        .publish("zincati", QoS::AtMostOnce, false, payload)
                        .await?;
                }
                Err(RecvError::Lagged(amount)) => {
                    log::info!("Lagged {amount} updates");
                }
                Err(RecvError::Closed) => return Ok(()),
            }
        }
    }

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

    async fn handle_msg(&self, publish: Publish) -> anyhow::Result<()> {
        let topic = &publish.topic;
        debug!("Command: {topic}");

        match topic.split('/').collect::<Vec<_>>().as_slice() {
            ["command", "inbox", _, "update"] => {
                let release = serde_json::from_slice(&publish.payload)?;
                self.update
                    .try_send(StartUpgrade(release))
                    .context("Triggering update")?;
            }
            _ => {
                bail!("Invalid command: {topic}");
            }
        }

        self.client.ack(&publish).await?;
        Ok(())
    }

    async fn handle_connected(&self) -> anyhow::Result<()> {
        self.client
            .subscribe("command/inbox//#", QoS::AtLeastOnce)
            .await?;

        // send last known state
        if let Some(state) = &*self.state.read().await {
            if let Ok(payload) = serde_json::to_vec(state) {
                let _ = self
                    .client
                    .try_publish("zincati", QoS::AtMostOnce, false, payload);
            }
        }

        Ok(())
    }
}
