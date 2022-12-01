//! MQTT helper functionality
//!
//! This is mostly copied over from Drogue Doppelgaenger. Might be worth externalizing.

use anyhow::Context;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use rumqttc::{MqttOptions, TlsConfiguration, Transport};
use rustls::{
    self,
    client::{NoClientSessionStorage, ServerCertVerified, ServerCertVerifier},
    Certificate, ClientConfig, Error, ServerName,
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MqttClient {
    pub host: String,
    pub port: u16,

    #[serde(default)]
    pub client_id: Option<String>,

    #[serde(default)]
    pub disable_tls: bool,
    #[serde(default)]
    pub insecure: bool,

    pub initial_reconnect_delay: Duration,
    pub keepalive: Duration,
}

impl TryFrom<MqttClient> for MqttOptions {
    type Error = anyhow::Error;

    fn try_from(config: MqttClient) -> Result<Self, Self::Error> {
        // must be between 1 and 23 alphanumeric characters: [MQTT-3.1.3-5]
        // some servers might allow more, but this isn't guaranteed.
        let client_id: String = config.client_id.unwrap_or_else(|| {
            thread_rng()
                .sample_iter(&Alphanumeric)
                .take(12)
                .map(char::from)
                .collect()
        });

        let mut opts = MqttOptions::new(client_id, config.host, config.port);

        opts.set_manual_acks(true).set_keep_alive(config.keepalive);

        if !config.disable_tls {
            opts.set_transport(Transport::Tls(setup_tls(config.insecure)?));
        }

        Ok(opts)
    }
}

pub struct InsecureServerCertVerifier;

impl ServerCertVerifier for InsecureServerCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }
}

/// Setup TLS with RusTLS and system certificates.
fn setup_tls(insecure: bool) -> anyhow::Result<TlsConfiguration> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().context("could not load platform certs")? {
        roots.add(&rustls::Certificate(cert.0))?;
    }

    let mut client_config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();

    if insecure {
        log::warn!("Disabling TLS validation. Do not use this in production!");
        client_config
            .dangerous()
            .set_certificate_verifier(Arc::new(InsecureServerCertVerifier));
    }

    client_config.session_storage = Arc::new(NoClientSessionStorage {});

    Ok(TlsConfiguration::Rustls(Arc::new(client_config)))
}
