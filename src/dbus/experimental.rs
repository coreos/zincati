//! Experimental interface.

use crate::update_agent::{LastRefresh, UpdateAgent};
use actix::Addr;
use futures::prelude::*;
use tokio::runtime::Runtime;
use zbus::{fdo, interface};

/// Experimental interface for testing.
pub(crate) struct Experimental {
    pub(crate) agent_addr: Addr<UpdateAgent>,
}

#[interface(name = "org.coreos.zincati.Experimental")]
impl Experimental {
    /// Just a test method.
    fn moo(&self, talkative: bool) -> String {
        if talkative {
            String::from("Moooo mooo moooo!")
        } else {
            String::from("moo.")
        }
    }

    /// Get update_agent actor's last refresh time.
    fn last_refresh_time(&self) -> fdo::Result<i64> {
        let msg = LastRefresh {};
        let refresh_time_fut = self.agent_addr.send(msg).map_err(|e| {
            let err_msg = format!("failed to get last refresh time from agent actor: {}", e);
            log::error!("LastRefreshTime D-Bus method call: {}", err_msg);
            fdo::Error::Failed(err_msg)
        });

        Runtime::new()
            .map_err(|e| {
                let err_msg = format!("failed to create runtime to execute future: {}", e);
                log::error!("{}", err_msg);
                fdo::Error::Failed(err_msg)
            })
            .and_then(|runtime| runtime.block_on(refresh_time_fut))
    }
}
