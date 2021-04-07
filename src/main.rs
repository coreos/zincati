//! Agent for Fedora CoreOS auto-updates.

#![deny(missing_debug_implementations)]
#![deny(missing_docs)]

#[macro_use(fail_point)]
extern crate fail;
#[macro_use]
extern crate prometheus;

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
/// Logic for weekly maintenance windows.
mod weekly;

use structopt::clap::crate_name;
use structopt::StructOpt;

/// Binary entrypoint, for all CLI subcommands.
fn main() {
    let exit_code = run();
    std::process::exit(exit_code);
}

/// Run till completion or failure, pretty-printing termination errors if any.
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
    match cli_opts.run() {
        Ok(_) => libc::EXIT_SUCCESS,
        Err(e) => {
            log_error_chain(e);
            libc::EXIT_FAILURE
        }
    }
}

/// Pretty-print a chain of errors, as a series of error-priority log messages.
fn log_error_chain(err_chain: failure::Error) {
    let mut chain_iter = err_chain.iter_chain();
    let top_err = match chain_iter.next() {
        Some(e) => e.to_string(),
        None => "(unspecified failure)".to_string(),
    };
    log::error!("critical error: {}", top_err);
    for err in chain_iter {
        log::error!(" -> {}", err);
    }
}
