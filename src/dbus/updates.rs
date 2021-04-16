//! Updates interface for ushering the update agent to various states.

use crate::update_agent::{RefreshTick, RefreshTickCommand, UpdateAgent, UpdateAgentState};
use actix::Addr;
use failure::Error;
use fdo::Error::Failed;
use futures::prelude::*;
use tokio::runtime::Runtime;
use zbus::{dbus_interface, fdo};

/// Updates interface for checking for and finalizing updates.
pub(crate) struct Updates {
    pub(crate) agent_addr: Addr<UpdateAgent>,
}

impl Updates {
    /// Send msg to the update agent actor and wait for the returned future to resolve.
    fn send_msg_to_agent(
        &self,
        msg: RefreshTick,
    ) -> Result<Result<UpdateAgentState, Error>, fdo::Error> {
        let refresh_time_fut = self.agent_addr.send(msg).map_err(|e| {
            let err_msg = format!("failed to get last refresh time from agent actor: {}", e);
            log::error!("LastRefreshTime D-Bus method call: {}", err_msg);
            Failed(err_msg)
        });

        Runtime::new()
            .map_err(|e| {
                let err_msg = format!("failed to create runtime to execute future: {}", e);
                log::error!("{}", err_msg);
                Failed(err_msg)
            })
            .and_then(|mut runtime| runtime.block_on(refresh_time_fut))
    }
}

#[dbus_interface(name = "org.coreos.zincati.Updates")]
impl Updates {
    /// Check for update immediately.
    fn check_update(&self) -> fdo::Result<Vec<String>> {
        let msg = RefreshTick {
            command: RefreshTickCommand::CheckUpdate,
        };

        self.send_msg_to_agent(msg).and_then(|res| match res {
            Ok(state) => match state {
                UpdateAgentState::NoNewUpdate => Ok(vec![]),
                UpdateAgentState::UpdateAvailable((release, _)) => Ok(vec![release.version]),
                _ => {
                    let err_msg = "update agent reached unexpected state after update check";
                    log::error!("CheckUpdate D-Bus method call: {}", err_msg);
                    Err(Failed(String::from(err_msg)))
                }
            },
            Err(e) => Err(Failed(format!("{}", e))),
        })
    }

    /// Finalize update immediately.
    fn finalize_update(&self, force: bool) -> fdo::Result<Vec<String>> {
        let msg = RefreshTick {
            command: RefreshTickCommand::FinalizeUpdate { force },
        };

        self.send_msg_to_agent(msg).and_then(|res| match res {
            Ok(state) => match state {
                UpdateAgentState::UpdateStaged(_) => {
                    Err(Failed(String::from("update finalization attempt failed")))
                }
                UpdateAgentState::UpdateFinalized(release) => Ok(vec![release.version]),
                _ => {
                    let err_msg =
                        "update agent reached unexpected state after finalization attempt";
                    log::error!("FinalizeUpdate D-Bus method call: {}", err_msg);
                    Err(Failed(String::from(err_msg)))
                }
            },
            Err(e) => Err(Failed(format!("{}", e))),
        })
    }
}
