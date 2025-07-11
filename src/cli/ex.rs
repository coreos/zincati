//! Logic for the ex subcommand.

use super::ensure_user;
use anyhow::Result;
use clap::Subcommand;
use fn_error_context::context;
use zbus::proxy;

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Replies different cow-speak depending on whether the
    /// talkative flag is set.
    #[command(name = "moo")]
    Moo {
        #[arg(long)]
        talkative: bool,
    },
    /// Get last refresh time of update agent actor's state.
    #[command(name = "last-refresh-time")]
    LastRefreshTime,
}

impl Cmd {
    /// `ex` subcommand entry point.
    #[context("failed to run `ex` subcommand")]
    pub(crate) fn run(self) -> Result<()> {
        ensure_user(
            "root",
            "ex subcommand must be run as `root` user, \
             and should only be used for testing purposes",
        )?;
        let connection = zbus::blocking::Connection::system()?;
        let proxy = ExperimentalProxyBlocking::new(&connection)?;
        match self {
            Cmd::Moo { talkative } => {
                println!("{}", proxy.moo(talkative)?);
                Ok(())
            }
            Cmd::LastRefreshTime => {
                println!("{}", proxy.last_refresh_time()?);
                Ok(())
            }
        }
    }
}

#[proxy(
    interface = "org.coreos.zincati.Experimental",
    default_service = "org.coreos.zincati",
    default_path = "/org/coreos/zincati"
)]
trait Experimental {
    /// LastRefreshTime method
    fn last_refresh_time(&self) -> zbus::Result<i64>;

    /// Moo method
    fn moo(&self, talkative: bool) -> zbus::Result<String>;
}
