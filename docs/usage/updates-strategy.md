---
title: Updates strategy
parent: Usage
layout: default
---

# Updates strategy

To minimize service disruption, Zincati allows administrators to control when machines are allowed to reboot and finalize auto-updates.

Several updates strategies are supported, which can be configured at runtime via configuration snippets as shown below.
If not otherwise configured, the default updates strategy resolves to `immediate`.

# Immediate strategy

The simplest updates strategy consists of minimal logic to immediately finalize an update as soon as it is staged locally.

For configuration purposes, such strategy is labeled `immediate` and takes no additional configuration parameters.

This strategy can be enabled via a configuration snippet like the following:

```toml
[updates]
strategy = "immediate"
```

The `immediate` strategy is an aggressive finalization method which is biased towards finalizing updates as soon as possible, and it is only aware of node-local state.

Such an approach is only recommended for environments where temporary service interruption are not problematic, or there is no need for more complex reboot scheduling.

# Lock-based strategy

In case of a fleet of machines grouped into a cluster, it is often required to orchestrate reboots so that hosted services are not disrupted when single nodes are rebooting to finalize updates.
In this case it is helpful to have an external orchestrator managing reboots cluster-wide, and having each machine trying to lock (and unlock) a reboot slot with the centralized lock-manager.

Several distributed databases and lock-managers exist for such purpose, each one with a specific remote API for clients and a variety of transport mechanisms.
Zincati does not mandate any specific lock-manager or database, but instead it uses a simple HTTP-based protocol modeling a distributed counting semaphore with recursive locking, called [FleetLock][fleet_lock], 

In short, it consists of two operations:
 * lock: before rebooting, a reboot slot must be locked (and confirmed) by the lock-manager.
 * unlock: after rebooting, any reboot slot owned by the node must be unlocked (and confirmed) by the lock-manager before proceeding further.

This protocol is not coupled to any specific backend, and can be implemented on top of any suitable database.
As an example, [airlock] is a free-software project which implements such protocol on top of [etcd3].

For configuration purposes, such strategy is labeled `fleet_lock` and takes the following configuration parameters:
 * `base_url` (string, mandatory, non-empty): the base URL for the FleetLock service.

This strategy can be enabled via a configuration snippet like the following:

```toml
[updates]
strategy = "fleet_lock"

[updates.fleet_lock]
base_url = "http://example.com/fleet_lock/"
```

The `fleet_lock` strategy is a conservative method which is biased towards avoiding service disruptions, but it requires an external component which is aware of cluster-wide state.

Such an approach is only recommended where nodes are already grouped into an orchestrated cluster, which can thus provide better overall scheduling decisions.

[fleet_lock]: https://github.com/coreos/airlock/pull/1 
[airlock]: https://github.com/coreos/airlock
[etcd3]: https://etcd.io/

# Periodic strategy

The `periodic` strategy allows Zincati to only reboot for updates during certain timeframes, also known as "maintenance windows" or "reboot windows".
Outside of those maintenance windows, reboots are not automatically performed and auto-updates are staged and held until the next available window.

Reboot windows recur on a weekly basis, and can be defined in any arbitrary order and length. Their individual length must be greater than zero.
To avoid timezone-related skews in a fleet of machines, all maintenance windows are defined in UTC dates and times.

Periodic reboot windows can be configured and enabled in the following way:

```toml
[updates]
strategy = "periodic"

[[updates.periodic.window]]
days = [ "Sat", "Sun" ]
start_time = "23:30"
length_minutes = 60

[[updates.periodic.window]]
days = [ "Wed" ]
start_time = "01:00"
length_minutes = 30
```

The above configuration would result in three maintenance windows during which Zincati is allowed to reboot the machine for updates:
 * 60 minutes starting at 23:30 UTC on Saturday night, and ending at 00:30 UTC on Sunday morning
 * 60 minutes starting at 23:30 UTC on Sunday night, and ending at 00:30 UTC on Monday morning
 * 30 minutes starting at 01:00 UTC on Wednesday morning, and ending at 01:30 UTC on Wednesday morning

Reboot windows can be separately configured in multiple snippets, as long as each `updates.periodic.window` entry contains all the required properties:
 * `days`: an array of weekdays (C locale), either in full or abbreviated (first three letters) form
 * `start_time`: window starting time, in `hh:mm` ISO 8601 format
 * `length_minutes`: non-zero window duration, in minutes

For convenience, multiple entries can be defined with overlapping times, and each window definition is allowed to cross day and week boundaries (wrapping to the next day).
