---
layout: default
parent: Usage
---

# Logging

Zincati supports logging at multiple levels (trace, debug, info, warning, error). Usually only log messages at or above `info` level are emitted.
Log verbosity can be increased by passing multiple `-v` flags as command-line arguments.

## Tweaking service verbosity

Logging verbosity for Zincati service can be tweaked via systemd drop-in files.

For example, debug logging (`-vv`) can be enabled by creating a drop-in file `/etc/systemd/system/zincati.service.d/10-verbosity.conf` with the following contents:

```
[Service]
Environment=ZINCATI_VERBOSITY="-vv"
```
