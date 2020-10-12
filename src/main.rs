//! Update agent.

#![deny(missing_debug_implementations)]
#![deny(missing_docs)]

#[macro_use(fail_point)]
extern crate fail;
#[macro_use]
extern crate prometheus;

// Cincinnati client.
mod cincinnati;
/// Command-line options.
mod cli;
/// File-based configuration.
mod config;
/// FleetLock client.
mod fleet_lock;
/// Agent identity.
mod identity;
/// Metrics service.
mod metrics;
/// rpm-ostree client.
mod rpm_ostree;
/// Update strategies.
mod strategy;
/// Update agent.
mod update_agent;
/// Logic for weekly maintenance windows.
mod weekly;

use actix::Actor;
use failure::{bail, ResultExt};
use log::{info, trace};
use prometheus::IntGauge;
use std::fs::Permissions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::{fs, io};
use structopt::clap::{crate_name, crate_version};
use structopt::StructOpt;

lazy_static::lazy_static! {
    static ref PROCESS_START_TIME: IntGauge = register_int_gauge!(opts!(
        "process_start_time_seconds",
        "Start time of the process since unix epoch in seconds."
    )).unwrap();
}

/// Binary entrypoint, for all CLI subcommands.
fn main() {
    let exit_code = run();
    std::process::exit(exit_code);
}

// Run till completion or failure, pretty-printing termination errors if any.
fn run() -> i32 {
    // Parse command-line options.
    let cli_opts = cli::CliOptions::from_args();

    // Setup logging.
    env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .format_module_path(false)
        .filter(Some(crate_name!()), cli_opts.loglevel())
        .init();

    // Dispatch CLI subcommand.
    let exit = match cli_opts.cmd {
        cli::CliCommand::Agent => run_agent(),
        cli::CliCommand::Deadend { reason } => write_deadend_release_info(reason),
    };

    match exit {
        Ok(_) => libc::EXIT_SUCCESS,
        Err(e) => {
            let mut err_chain = e.iter_chain();
            let top_err = match err_chain.next() {
                Some(e) => e.to_string(),
                None => "(unspecified failure)".to_string(),
            };
            log::error!("critical error: {}", top_err);
            for err in err_chain {
                log::error!(" -> {}", err);
            }

            libc::EXIT_FAILURE
        }
    }
}

/// Agent subcommand entry-point.
fn run_agent() -> failure::Fallible<()> {
    info!(
        "starting update agent ({} {})",
        crate_name!(),
        crate_version!()
    );

    let settings =
        config::Settings::assemble().context("failed to assemble configuration settings")?;
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
    let _addr = agent.start();

    trace!("starting actor system");
    sys.run().context("agent failed")?;

    Ok(())
}

/// Deadend subcommand entry point.
fn write_deadend_release_info(reason: Option<String>) -> failure::Fallible<()> {
    if reason.is_some() {
        // Avoid showing partially-written messages using tempfile and
        // persist (rename).
        let mut f = tempfile::Builder::new()
            .prefix(".deadend.")
            .suffix(".motd.partial")
            // Create the tempfile in the same directory as the final MOTD,
            // to ensure proper SELinux labels are applied to the tempfile
            // before renaming.
            .tempfile_in("/run/motd.d")
            .with_context(|e| format!("failed to create temporary MOTD file: {}", e))?;
        // Set correct permissions of the temporary file, before moving to
        // the destination (`tempfile` creates files with mode 0600).
        fs::set_permissions(f.path(), Permissions::from_mode(0o644))
            .with_context(|e| format!("failed to set permissions of temporary MOTD file: {}", e))?;

        if let Some(reason) = reason {
            writeln!(
                f,
                "This release is a dead-end and won't auto-update: {}",
                reason
            )
            .with_context(|e| format!("failed to write MOTD: {}", e))?;
        }

        f.persist("/run/motd.d/85-zincati-deadend.motd")
            .with_context(|e| format!("failed to persist temporary MOTD file: {}", e))?;
    } else if let Err(e) = std::fs::remove_file("/run/motd.d/85-zincati-deadend.motd") {
        if e.kind() != io::ErrorKind::NotFound {
            bail!("failed to remove dead-end release info file: {}", e);
        }
    }
    Ok(())
}
