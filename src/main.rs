//! Update agent.

#![deny(missing_debug_implementations)]
#![deny(missing_docs)]

/// Command-line options.
mod cli;
/// File-based configuration.
mod config;
/// Agent identity.
mod identity;
/// Update strategies.
mod strategy;

use failure::ResultExt;
use log::{debug, info, trace};
use structopt::clap::{crate_name, crate_version};
use structopt::StructOpt;

fn main() -> failure::Fallible<()> {
    // Parse command-line options.
    let cli_opts = cli::CliOptions::from_args();

    // Setup logging.
    env_logger::Builder::from_default_env()
        .default_format_timestamp(false)
        .default_format_module_path(false)
        .filter(Some(crate_name!()), cli_opts.loglevel())
        .try_init()
        .context("failed to initialize logging")?;

    // Dispatch CLI subcommand.
    match cli_opts.cmd {
        cli::CliCommand::Agent => run_agent(),
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
        "agent running on node '{}' in group '{}'",
        settings.identity.node_uuid.lower_hex(),
        settings.identity.group
    );

    trace!("creating actor system");
    let sys = actix::System::new(crate_name!());

    // TODO(lucab): parse configuration files and run agent.

    debug!("starting actor system");
    sys.run().context("agent failed")?;

    Ok(())
}
