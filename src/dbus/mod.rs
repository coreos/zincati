//! D-Bus service actor.

mod experimental;
use experimental::Experimental;

use crate::update_agent::UpdateAgent;
use actix::prelude::*;
use actix::Addr;
use anyhow::Result;
use core::convert::TryFrom;
use fn_error_context::context;
use log::trace;
use zbus::fdo;
use zvariant::ObjectPath;

pub struct DBusService {
    agent_addr: Addr<UpdateAgent>,
}

impl DBusService {
    /// Create new DBusService
    fn new(agent_addr: Addr<UpdateAgent>) -> DBusService {
        DBusService { agent_addr }
    }

    /// Start the threadpool for DBusService actor.
    pub(crate) fn start(threads: usize, agent_addr: Addr<UpdateAgent>) -> Addr<Self> {
        SyncArbiter::start(threads, move || DBusService::new(agent_addr.clone()))
    }

    #[context("failed to start object server")]
    fn start_object_server(&mut self) -> Result<()> {
        let connection = zbus::Connection::new_system()?;

        fdo::DBusProxy::new(&connection)?.request_name(
            "org.coreos.zincati",
            fdo::RequestNameFlags::ReplaceExisting.into(),
        )?;

        let mut object_server = zbus::ObjectServer::new(&connection);
        let experimental_interface = Experimental {
            agent_addr: self.agent_addr.clone(),
        };
        object_server.at(
            &ObjectPath::try_from("/org/coreos/zincati")?,
            experimental_interface,
        )?;

        loop {
            if let Err(err) = object_server.try_handle_next() {
                log::warn!("{}", err);
            }
        }
    }
}

impl Actor for DBusService {
    type Context = SyncContext<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        trace!("D-Bus service actor started");

        if let Err(err) = self.start_object_server() {
            log::error!("failed to start D-Bus service actor: {}", err);
        }
    }
}
