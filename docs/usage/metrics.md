# Metrics

Zincati tracks and exposes some of its internal metrics, in order to ease monitoring tasks across a large fleet of nodes.

Metrics are collected and exported according to [Prometheus] [textual format](prom-text), over a local endpoint.

[Prometheus]: https://prometheus.io/
[prom-text]: https://prometheus.io/docs/instrumenting/exposition_formats/

## Gathering metrics

To gather metrics from a locally running Zincati instance, it is sufficient to connect and read from the Unix-domain socket located at `/run/zincati/public/metrics.promsock`.

For example, manual inspection can be performed via `ncat`:

```
$ sudo socat - UNIX-CONNECT:/run/zincati/public/metrics.promsock

# HELP zincati_update_agent_last_refresh_timestamp UTC timestamp of update-agent last refresh tick.
# TYPE zincati_update_agent_last_refresh_timestamp gauge
zincati_update_agent_last_refresh_timestamp 1563360122
# HELP zincati_update_agent_latest_state_change_timestamp UTC timestamp of update-agent last state change.
# TYPE zincati_update_agent_latest_state_change_timestamp gauge
zincati_update_agent_latest_state_change_timestamp 1563360122
# HELP zincati_update_agent_updates_enabled Whether auto-updates logic is enabled.
# TYPE zincati_update_agent_updates_enabled gauge
zincati_update_agent_updates_enabled 1
[...]
```

Additionally, the local Unix-domain socket can be proxied to HTTP and exposed to Prometheus.
For an example of such setup, check the [local\_exporter] repository.

[local_exporter]: https://github.com/lucab/local_exporter
