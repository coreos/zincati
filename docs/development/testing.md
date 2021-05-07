---
layout: default
nav_order: 7
parent: Development
---

# Testing

## Unit Tests
Unit tests can be run using `make check` (via `cargo test`).

## External Kola Tests
[External Kola tests][kola-ext-tests] can be found in the `tests/kola/` directory.

### `server` Tests
The `tests/kola/server/` test directory contains tests that require access to a mock Cincinnati server. This test directory contains a Fedora CoreOS config that does the following:
- creates a `/var/www/` directory
- [sets up an HTTP server][kolet-httpd] at `localhost` listening on port `80` serving files from the `/var/www/` directory
- configures Zincati to use `localhost` as its Cincinnati base URL
- adds a systemd dropin to set Zincati's journal log verbosity to max (`-vvvv`)

Tests place mock release graphs in `/var/www/` for Zincati to fetch.

### Running the Tests
A built Fedora CoreOS image is required; it is recommended to use the CoreOS Assembler's [`build-fast` command][cosa-build-fast] for faster iteration.

To run the tests, specify the path to your Zincati project directory and which tests to run using `kola run`'s `-E` option.

Example (run all tests):
```
kola run --qemu-image fastbuild-fedora-coreos-zincati-qemu.qcow2 -E /path/to/zincati/ 'ext.zincati.*'
```

Example (run only the `server` tests):
```
kola run --qemu-image fastbuild-fedora-coreos-zincati-qemu.qcow2 -E /path/to/zincati/ 'ext.zincati.server.*'
```

### Adding Tests
Refer to kola external tests' [README][kola-ext-quick-start] for instructions on adding additional tests

[kolet-httpd]: https://github.com/coreos/coreos-assembler/blob/main/docs/kola/external-tests.md#http-server
[cosa-build-fast]: https://coreos.github.io/coreos-assembler/kola/external-tests/#fast-build-and-iteration-on-your-projects-tests
[kola-ext-tests]: https://coreos.github.io/coreos-assembler/kola/external-tests/
[kola-ext-quick-start]: https://coreos.github.io/coreos-assembler/kola/external-tests/#quick-start
