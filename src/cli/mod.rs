//! Command-Line Interface (CLI) logic.

mod agent;
mod deadend;
mod ex;

use anyhow::Result;
use clap::{ArgAction, Parser};
use log::LevelFilter;
use users::get_current_username;

/// CLI configuration options.
#[derive(Debug, Parser)]
pub(crate) struct CliOptions {
    /// Verbosity level (higher is more verbose).
    #[arg(action = ArgAction::Count, short = 'v', global = true)]
    verbosity: u8,

    /// CLI sub-command.
    #[clap(subcommand)]
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
    pub(crate) fn run(self) -> Result<()> {
        match self.cmd {
            CliCommand::Agent => agent::run_agent(),
            CliCommand::DeadendMotd(cmd) => cmd.run(),
            CliCommand::Ex(cmd) => cmd.run(),
        }
    }
}

/// CLI sub-commands.
#[derive(Debug, Parser)]
#[command(rename_all = "kebab-case")]
pub(crate) enum CliCommand {
    /// Long-running agent for auto-updates.
    Agent,
    /// Set or unset deadend MOTD state.
    #[command(hide = true, subcommand)]
    DeadendMotd(deadend::Cmd),
    /// Print update agent state's last refresh time.
    #[command(hide = true, subcommand)]
    Ex(ex::Cmd),
}

/// Return Error with msg if not run by user.
fn ensure_user(user: &str, msg: &str) -> Result<()> {
    if let Some(uname) = get_current_username() {
        if uname == user {
            return Ok(());
        }
    }

    anyhow::bail!("{}", msg)
}
