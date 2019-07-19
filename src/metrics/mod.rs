//! Metrics endpoint over a Unix-domain socket.

use actix::prelude::*;
use failure::{Fallible, ResultExt};
use std::os::unix::net as std_net;
use tokio::net as tokio_net;
use tokio::prelude::*;
use tokio::reactor;

/// Unix socket path.
static SOCKET_PATH: &str = "/run/zincati/private/metrics.promsock";

/// Metrics exposition service.
pub struct MetricsService {
    listener: std_net::UnixListener,
}

impl MetricsService {
    /// Create metrics service and bind to the Unix-domain socket.
    pub fn bind_socket() -> Fallible<Self> {
        let _ = std::fs::remove_file(SOCKET_PATH);
        let listener =
            std_net::UnixListener::bind(SOCKET_PATH).context("failed to bind metrics service")?;
        Ok(Self { listener })
    }

    /// Gather metrics from the default registry and encode them in textual format.
    fn prometheus_text_encode() -> Fallible<Vec<u8>> {
        use prometheus::Encoder;

        let metric_families = prometheus::gather();
        let encoder = prometheus::TextEncoder::new();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(buffer)
    }
}

/// Incoming Unix-domain socket connection.
#[derive(Message)]
struct Connection {
    stream: tokio_net::UnixStream,
}

impl Actor for MetricsService {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let listener = self
            .listener
            .try_clone()
            .expect("failed to clone metrics listener");
        let async_listener =
            tokio_net::UnixListener::from_std(listener, &reactor::Handle::default())
                .expect("failed to create async metrics listener");

        let connections = async_listener
            .incoming()
            .map(|stream| Connection { stream });
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

impl StreamHandler<Connection, std::io::Error> for MetricsService {
    fn handle(&mut self, item: Connection, ctx: &mut Context<MetricsService>) {
        let (_, sink) = item.stream.split();
        let mut wr = actix::io::Writer::new(sink, ctx);
        if let Ok(metrics) = MetricsService::prometheus_text_encode() {
            wr.write(&metrics);
        }
        wr.close();
    }
}
