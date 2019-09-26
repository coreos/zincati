# Zincati

[![Build status](https://travis-ci.org/coreos/zincati.svg?branch=master)](https://travis-ci.org/coreos/zincati)
[![crates.io](https://img.shields.io/crates/v/zincati.svg)](https://crates.io/crates/zincati)

Zincati is an auto-update agent for Fedora CoreOS hosts.

It works as a client for [Cincinnati] and [rpm-ostree], taking care of automatically updating/rebooting machines.

Features:
 * Agent for [continuous auto-updates][auto-updates], with support for phased rollouts
 * [Configuration][configuration] via TOML dropins and overlaid directories
 * Multiple [update strategies][updates-strategy] for finalization/reboot
 * Internal [metrics][metrics] exposed over a local endpoint
 * [Logging][logging] with configurable priority levels
 * Support for complex update-graphs in Cincinnati format
 * Support for cluster-wide reboot orchestration, via an external lock-manager

![cluster reboot graph](./docs/images/metrics.png)

[Cincinnati]: https://github.com/openshift/cincinnati
[rpm-ostree]: https://github.com/coreos/rpm-ostree

[auto-updates]: ./docs/usage/auto-updates.md
[configuration]: ./docs/usage/configuration.md
[updates-strategy]: ./docs/usage/updates-strategy.md
[metrics]: ./docs/usage/metrics.md
[logging]: ./docs/usage/logging.md
