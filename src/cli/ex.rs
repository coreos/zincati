//! Logic for the ex subcommand.

use super::ensure_user;
use failure::Fallible;
use structopt::StructOpt;
use zbus::dbus_proxy;

#[derive(Debug, StructOpt)]
pub enum Cmd {
    /// Replies different cow-speak depending on whether the
    /// talkative flag is set.
    #[structopt(name = "moo")]
    Moo {
        #[structopt(long)]
        talkative: bool,
    },
    /// Get last refresh time of update agent actor's state.
    #[structopt(name = "last-refresh-time")]
    LastRefreshTime,
}

impl Cmd {
    /// `ex` subcommand entry point.
    pub(crate) fn run(self) -> Fallible<()> {
        ensure_user(
            "root",
            "ex subcommand must be run as `root` user, \
             and should only be used for testing purposes",
        )?;
        let connection = zbus::Connection::new_system()?;
        let proxy = ExperimentalProxy::new(&connection)?;
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

#[dbus_proxy(
    interface = "org.coreos.zincati1.Experimental",
    default_service = "org.coreos.zincati1",
    default_path = "/org/coreos/zincati1"
)]
trait Experimental {
    /// LastRefreshTime method
    fn last_refresh_time(&self) -> zbus::Result<i64>;

    /// Moo method
    fn moo(&self, talkative: bool) -> zbus::Result<String>;
}
