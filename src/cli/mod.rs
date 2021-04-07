//! Command-Line Interface (CLI) logic.

mod agent;
mod deadend;
mod ex;

use log::LevelFilter;
use structopt::clap::AppSettings;
use structopt::StructOpt;
use users::get_current_username;

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

    /// Dispatch CLI subcommand.
    pub(crate) fn run(self) -> failure::Fallible<()> {
        match self.cmd {
            CliCommand::Agent => agent::run_agent(),
            CliCommand::DeadendMotd(cmd) => cmd.run(),
            CliCommand::Ex(cmd) => cmd.run(),
        }
    }
}

/// CLI sub-commands.
#[derive(Debug, StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub(crate) enum CliCommand {
    /// Long-running agent for auto-updates.
    Agent,
    /// Set or unset deadend MOTD state.
    #[structopt(setting = AppSettings::Hidden)]
    DeadendMotd(deadend::Cmd),
    /// Print update agent state's last refresh time.
    #[structopt(setting = AppSettings::Hidden)]
    Ex(ex::Cmd),
}

/// Return Error with msg if not run by user.
fn ensure_user(user: &str, msg: &str) -> failure::Fallible<()> {
    if let Some(uname) = get_current_username() {
        if uname == user {
            return Ok(());
        }
    }

    failure::bail!("{}", msg)
}
