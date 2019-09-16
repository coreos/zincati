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
[etcd3]: https://etcd.io/
