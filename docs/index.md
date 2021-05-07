---
layout: default
nav_order: 1
---

# Zincati

[![Build status](https://travis-ci.org/coreos/zincati.svg?branch=main)](https://travis-ci.org/coreos/zincati)
[![crates.io](https://img.shields.io/crates/v/zincati.svg)](https://crates.io/crates/zincati)

Zincati is an auto-update agent for Fedora CoreOS hosts.

It works as a client for [Cincinnati] and [rpm-ostree], taking care of automatically updating/rebooting machines.

Features:
 * Agent for [continuous auto-updates][auto-updates], with support for phased rollouts
 * [Configuration][configuration] via TOML dropins and overlaid directories
 * Multiple [update strategies][updates-strategy] for finalization/reboot
 * Local [maintenance windows][strategy-periodic] on a weekly schedule for planned upgrades
 * Internal [metrics][metrics] exposed over a local endpoint in Prometheus format
 * [Logging][logging] with configurable priority levels
 * Support for complex update-graphs via [Cincinnati protocol][cincinnati-protocol] (with rollout wariness, barriers, dead-ends and more)
 * Support for [cluster-wide reboot orchestration][strategy-fleetlock], via an external lock-manager

![cluster reboot graph](images/metrics.png){:width="720px"}

[Cincinnati]: https://github.com/openshift/cincinnati
[rpm-ostree]: https://github.com/coreos/rpm-ostree

[auto-updates]: usage/auto-updates
[configuration]: usage/configuration
[updates-strategy]: usage/updates-strategy
[strategy-periodic]: usage/updates-strategy#periodic-strategy
[metrics]: usage/metrics
[logging]: usage/logging
[cincinnati-protocol]: development/cincinnati/protocol
[strategy-fleetlock]: usage/updates-strategy#lock-based-strategy
