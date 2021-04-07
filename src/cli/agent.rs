//! Logic for the `agent` subcommand.

use super::ensure_user;
use crate::{config, dbus, metrics, rpm_ostree, update_agent};
use actix::Actor;
use failure::{Fallible, ResultExt};
use log::{info, trace};
use prometheus::IntGauge;
use structopt::clap::{crate_name, crate_version};

lazy_static::lazy_static! {
    static ref PROCESS_START_TIME: IntGauge = register_int_gauge!(opts!(
        "process_start_time_seconds",
        "Start time of the process since unix epoch in seconds."
    )).unwrap();
}

/// Agent subcommand entry-point.
pub(crate) fn run_agent() -> Fallible<()> {
    ensure_user("zincati", "update agent not running as `zincati` user")?;
    info!(
        "starting update agent ({} {})",
        crate_name!(),
        crate_version!()
    );

    let settings =
        config::Settings::assemble().context("failed to assemble configuration settings")?;
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
    let sys = actix::System::builder()
        .name(crate_name!())
        .stop_on_panic(true)
        .build();

    trace!("creating metrics service");
    let _metrics_addr = metrics::MetricsService::bind_socket()?.start();

    trace!("creating rpm-ostree client");
    let rpm_ostree_addr = rpm_ostree::RpmOstreeClient::start(1);

    trace!("creating update agent");
    let agent = update_agent::UpdateAgent::with_config(settings, rpm_ostree_addr)
        .context("failed to assemble update-agent from configuration settings")?;
    let agent_addr = agent.start();

    trace!("creating D-Bus service");
    let _dbus_service_addr = dbus::DBusService::start(1, agent_addr);

    trace!("starting actor system");
    sys.run().context("agent failed")?;

    Ok(())
}
