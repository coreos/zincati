//! Metrics endpoint over a Unix-domain socket.

use actix::prelude::*;
use anyhow::{bail, Context, Result};
use std::os::unix::net as std_net;
use std::path::Path;
use tokio::net as tokio_net;

/// Unix socket path.
static SOCKET_PATH: &str = "/run/zincati/public/metrics.promsock";

/// Metrics exposition service.
#[derive(Debug)]
pub struct MetricsService {
    listener: std_net::UnixListener,
}

impl MetricsService {
    /// Create metrics service and bind to the Unix-domain socket.
    pub fn bind_socket() -> Result<Self> {
        Self::bind_socket_at(SOCKET_PATH)
            .with_context(|| format!("failed to setup metrics service on '{}'", SOCKET_PATH))
    }

    pub(crate) fn bind_socket_at(path: impl AsRef<Path>) -> Result<Self> {
        if let Err(e) = std::fs::remove_file(path.as_ref()) {
            if e.kind() != std::io::ErrorKind::NotFound {
                bail!("failed to remove socket file: {}", e);
            }
        };
        let listener = std_net::UnixListener::bind(path.as_ref())
            .context("failed to bind metrics service to Unix socket'")?;
        Ok(Self { listener })
    }

    /// Gather metrics from the default registry and encode them in textual format.
    fn prometheus_text_encode() -> Result<Vec<u8>> {
        use prometheus::Encoder;

        let metric_families = prometheus::gather();
        let encoder = prometheus::TextEncoder::new();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(buffer)
    }
}

/// Incoming Unix-domain socket connection.
struct Connection {
    stream: tokio_net::UnixStream,
}

impl Message for Connection {
    type Result = ();
}

impl Actor for MetricsService {
    type Context = actix::Context<Self>;

    fn started(&mut self, ctx: &mut actix::Context<Self>) {
        let listener = self
            .listener
            .try_clone()
            .expect("failed to clone metrics listener");
        listener
            .set_nonblocking(true)
            .expect("failed to move metrics listener into nonblocking mode");
        let async_listener = tokio_net::UnixListener::from_std(listener)
            .expect("failed to create async metrics listener");

        // This uses manual stream unfolding in order to keep the async listener
        // alive for the whole duration of the stream.
        let connections = futures::stream::unfold(async_listener, |l| async move {
            loop {
                let next = l.accept().await;
                if let Ok((stream, _addr)) = next {
                    let conn = Connection { stream };
                    break Some((conn, l));
                }
            }
        });

        ctx.add_stream(connections);

        log::debug!(
            "started metrics service on Unix-domain socket '{}'",
            SOCKET_PATH
        );
    }
}

impl actix::io::WriteHandler<std::io::Error> for MetricsService {
    fn error(&mut self, _err: std::io::Error, _ctx: &mut Self::Context) -> Running {
        actix::Running::Continue
    }

    fn finished(&mut self, _ctx: &mut Self::Context) {}
}

impl StreamHandler<Connection> for MetricsService {
    fn handle(&mut self, item: Connection, ctx: &mut actix::Context<MetricsService>) {
        let mut wr = actix::io::Writer::new(item.stream, ctx);
        if let Ok(metrics) = MetricsService::prometheus_text_encode() {
            wr.write(&metrics);
        }
        wr.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_socket_at() {
        // Error path (EPERM or EISDIR).
        MetricsService::bind_socket_at("/proc").unwrap_err();

        let tmpdir = tempfile::tempdir().unwrap();
        let tmp_socket_path = tmpdir.path().join("test-socket");
        // Create a socket file and leave it behind on disk.
        let service = MetricsService::bind_socket_at(&tmp_socket_path).unwrap();
        drop(service);
        // Make sure that the next run can remove it and start normally.
        let service = MetricsService::bind_socket_at(&tmp_socket_path).unwrap();
        drop(service);
    }
}
