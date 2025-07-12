//! D-Bus service actor.

mod experimental;
use experimental::Experimental;

use crate::update_agent::UpdateAgent;
use actix::prelude::*;
use actix::Addr;
use anyhow::Result;
use fn_error_context::context;
use log::trace;
use zbus::blocking::{connection, Connection};

pub struct DBusService {
    agent_addr: Addr<UpdateAgent>,
    connection: Option<Connection>,
}

impl DBusService {
    /// Create new DBusService
    fn new(agent_addr: Addr<UpdateAgent>) -> DBusService {
        DBusService {
            agent_addr,
            connection: None,
        }
    }

    /// Start the threadpool for DBusService actor.
    pub(crate) fn start(threads: usize, agent_addr: Addr<UpdateAgent>) -> Addr<Self> {
        SyncArbiter::start(threads, move || DBusService::new(agent_addr.clone()))
    }

    #[context("failed to start object server")]
    fn start_object_server(&mut self) -> Result<Connection> {
        let connection = connection::Builder::system()?
            .allow_name_replacements(true)
            .replace_existing_names(true)
            .name("org.coreos.zincati")?
            .serve_at(
                "/org/coreos/zincati",
                Experimental {
                    agent_addr: self.agent_addr.clone(),
                },
            )?
            .build()?;

        Ok(connection)
    }
}

impl Actor for DBusService {
    type Context = SyncContext<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        trace!("D-Bus service actor started");

        if let Some(conn) = self.connection.take() {
            drop(conn);
        }

        match self.start_object_server() {
            Err(err) => log::error!("failed to start D-Bus service actor: {}", err),
            Ok(conn) => self.connection = Some(conn),
        };
    }
}
