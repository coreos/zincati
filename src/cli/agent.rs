//! Logic for the `agent` subcommand.

use super::ensure_user;
use crate::{config, dbus, metrics, rpm_ostree, update_agent, utils};
use actix::Actor;
use anyhow::{Context, Result};
use clap::{crate_name, crate_version};
use log::{info, trace};
use prometheus::IntGauge;
use tokio::runtime::Runtime;

lazy_static::lazy_static! {
    static ref PROCESS_START_TIME: IntGauge = register_int_gauge!(opts!(
        "process_start_time_seconds",
        "Start time of the process since unix epoch in seconds."
    )).unwrap();
}

/// Agent subcommand entry-point.
pub(crate) fn run_agent() -> Result<()> {
    ensure_user("zincati", "update agent not running as `zincati` user")?;
    info!(
        "starting update agent ({} {})",
        crate_name!(),
        crate_version!()
    );

    // Start a new dedicated signal handling thread in a new runtime.
    let signal_handling_rt = Runtime::new().unwrap();
    signal_handling_rt.spawn(async {
        use tokio::signal::unix::{signal, SignalKind};

        // Create stream of terminate signals.
        let mut stream = signal(SignalKind::terminate()).expect("failed to set SIGTERM handler");

        stream.recv().await;
        // Reset status text to empty string (default).
        utils::update_unit_status("");
        utils::notify_stopping();
        std::process::exit(0);
    });

    let settings = config::Settings::assemble()?;
    settings.refresh_metrics();
    info!(
        "agent running on node '{}', in update group '{}'",
        settings.identity.node_uuid.lower_hex(),
        settings.identity.group
    );

    // Expose process start timestamp.
    let start_time = chrono::Utc::now();
    PROCESS_START_TIME.set(start_time.timestamp());

    trace!("creating actor system");
    let sys = actix::System::new();

    let _drogue = sys.block_on(async {
        trace!("Creating services");

        #[cfg(feature = "drogue")]
        let drogue_config = settings.drogue.clone();

        trace!("creating metrics service");
        let _metrics_addr = metrics::MetricsService::bind_socket()?.start();

        trace!("creating rpm-ostree client");
        let rpm_ostree_addr = rpm_ostree::RpmOstreeClient::start(1);

        trace!("creating update agent");
        let agent = update_agent::UpdateAgent::with_config(settings, rpm_ostree_addr);
        let agent_addr = agent.start();

        trace!("creating D-Bus service");
        let _dbus_service_addr = dbus::DBusService::start(1, agent_addr.clone());

        #[cfg(feature = "drogue")]
        trace!("starting Drogue IoT agent");
        #[cfg(feature = "drogue")]
        let drogue = crate::drogue::Agent::start(drogue_config, agent_addr)?;

        Ok::<_, anyhow::Error>(drogue)
    })?;

    trace!("starting actor system");
    sys.run().context("agent failed")?;

    Ok(())
}
