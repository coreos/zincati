//! Command-line interface (CLI) logic.

use log::LevelFilter;
use structopt::StructOpt;

/// CLI configuration options.
#[derive(Debug, StructOpt)]
pub(crate) struct CliOptions {
    /// Verbosity level (higher is more verbose).
    #[structopt(short = "v", parse(from_occurrences), global = true)]
    verbosity: u8,

    /// CLI sub-command.
    #[structopt(subcommand)]
    pub(crate) cmd: CliCommand,
}

impl CliOptions {
    /// Returns the log-level set via command-line flags.
    pub(crate) fn loglevel(&self) -> LevelFilter {
        match self.verbosity {
            0 => LevelFilter::Warn,
            1 => LevelFilter::Info,
            2 => LevelFilter::Debug,
            _ => LevelFilter::Trace,
        }
    }
}

/// CLI sub-commands.
#[derive(Debug, StructOpt)]
pub(crate) enum CliCommand {
    #[structopt(name = "agent")]
    Agent,
}
