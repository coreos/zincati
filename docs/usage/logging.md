---
layout: default
parent: Usage
---

# Logging

Zincati supports logging at multiple levels (trace, debug, info, warning, error). Usually only log messages at or above warning level are emitted.
Log verbosity can be increased by passing multiple `-v` flags as command-line arguments.

## Tweaking agent verbosity

By default, the Zincati agent is started with info level logging enabled (i.e. `-v`). However, logging verbosity can be freely tweaked via systemd drop-in files.

For example, debug logging (`-vv`) can be enabled by creating a drop-in file at `/etc/systemd/system/zincati.service.d/10-verbosity.conf` with the following contents:

```
[Service]
Environment=ZINCATI_VERBOSITY="-vv"
```

The maximum level (`-vvv`) equates to trace and can be very verbose. It is only meant for development/debugging and for short timespans.
It is recommended to not use the trace log level in production or for long periods of time as it reduces the signal-to-noise ratio and can easily saturate further log-persisting systems.

## Inspecting logs

By default Zincati runs as a systemd service, and its log messages are captured by systemd-journald.

Most recent logs can be inspected via `sudo journalctl -b 0 -e -u zincati.service`. The resulting output may look like this:

```
-- Logs begin at Sat 2020-09-12 16:12:13 UTC, end at Wed 2020-09-30 12:52:05 UTC. --
Sep 23 10:48:27 localhost systemd[1]: Started Zincati Update Agent.
Sep 23 10:48:27 localhost zincati[678]: [INFO ] starting update agent (zincati 0.0.12)
Sep 23 10:48:34 localhost zincati[678]: [INFO ] Cincinnati service: https://updates.coreos.fedoraproject.org
Sep 23 10:48:34 localhost zincati[678]: [INFO ] agent running on node '<ID>', in update group '<GROUP>'
Sep 23 10:48:34 localhost zincati[678]: [INFO ] initialization complete, auto-updates logic enabled
...
```

Optionally, `journalctl` allows to follow log messages emitted in real time by additionally passing a `-f` flag.
