//! Agent for Fedora CoreOS auto-updates.

#![deny(missing_debug_implementations)]
#![deny(missing_docs)]

#[macro_use(fail_point)]
extern crate fail;
#[macro_use]
extern crate prometheus;
extern crate core;

// Cincinnati client.
mod cincinnati;
mod cli;
/// File-based configuration.
mod config;
/// D-Bus service.
mod dbus;
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
/// Utility functions.
mod utils;
/// Logic for weekly maintenance windows.
mod weekly;

/// Drogue IoT agent.
#[cfg(feature = "drogue")]
mod drogue;

use clap::{crate_name, Parser};

/// Binary entrypoint, for all CLI subcommands.
fn main() {
    let exit_code = run();
    std::process::exit(exit_code);
}

/// Run till completion or failure, pretty-printing termination errors if any.
fn run() -> i32 {
    // Parse command-line options.
    let cli_opts = cli::CliOptions::parse();

    // Setup logging.
    env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .format_module_path(false)
        .filter(Some(crate_name!()), cli_opts.loglevel())
        .init();

    // Dispatch CLI subcommand.
    match cli_opts.run() {
        Ok(_) => libc::EXIT_SUCCESS,
        Err(e) => {
            log_error_chain(&e);
            if e.root_cause()
                .downcast_ref::<crate::rpm_ostree::FatalError>()
                .is_some()
            {
                7
            } else {
                libc::EXIT_FAILURE
            }
        }
    }
}

/// Pretty-print a chain of errors, as a series of error-priority log messages.
fn log_error_chain(err_chain: &anyhow::Error) {
    let mut chain_iter = err_chain.chain();
    let top_err = match chain_iter.next() {
        Some(e) => e.to_string(),
        None => "(unspecified failure)".to_string(),
    };
    log::error!("error: {}", top_err);
    for err in chain_iter {
        log::error!(" -> {}", err);
    }
}
