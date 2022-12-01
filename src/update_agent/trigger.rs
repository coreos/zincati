/// The way updates are controlled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Triggered by polling cincinnati for new updates.
    Cincinnati,
    /// Triggered by a remote command through the Drogue IoT MQTT endpoint.
    #[cfg(feature = "drogue")]
    Drogue,
}
