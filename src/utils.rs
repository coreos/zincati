//! Miscellaneous utility functions.

use libsystemd::daemon::{notify, NotifyState};
use log::info;

/// Helper function to update unit's status text.
pub(crate) fn update_unit_status(status: &str) {
    sd_notify(&[NotifyState::Status(status.to_string())]);
}

/// Helper function to notify the service manager that Zincati start up is finished and
/// configuration is loaded.
pub(crate) fn notify_ready() {
    sd_notify(&[NotifyState::Ready]);
}

/// Helper function to notify the service manager that Zincati is stopping.
pub(crate) fn notify_stopping() {
    sd_notify(&[NotifyState::Stopping]);
}

/// Helper function to send notifications to the service manager about service status changes.
/// Log errors if unsuccessful.
fn sd_notify(state: &[NotifyState]) {
    info!("Notify: {state:?}");

    match notify(false, state) {
        Err(e) => log::error!(
            "failed to notify service manager of service status change: {}",
            e
        ),
        Ok(sent) => {
            if !sent {
                log::error!("status notifications not supported for this service");
            }
        }
    }
}
