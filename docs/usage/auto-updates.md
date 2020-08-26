---
layout: default
parent: Usage
---

# Auto-updates

Available updates are discovered by periodically polling a [Cincinnati] server.
Once available, they are automatically applied via [rpm-ostree] and a machine reboot.

[Cincinnati]: https://github.com/openshift/cincinnati
[rpm-ostree]: https://github.com/projectatomic/rpm-ostree

## Phased rollouts, client wariness, canaries

Once a new update payload is officially released, Zincati will eventually detect and apply the update automatically.

However, there is no strict guarantee on the timing for an individual node to detect a new release, as the server will try to spread updates over a controlled timeframe.

This mechanism is called "phased rollout" and is meant to help release engineers and administrators in performing gradual updates and catching last-minute issues before they propagate to a large number of machines.

Phased rollouts are orchestrated by the Cincinnati backend, by adjusting over time the percentage of clients to which an update is offered.
Clients do not usually need any additional setup to leverage phased rollouts.
By default, the Cincinnati backend dynamically assigns a specific rollout score to each client.

However, clients can provide a "rollout wariness" hint to the server, in order to specify how eager they are to receive new updates.

The rollout wariness hint is configurable through the `rollout_wariness` parameter, as a floating point number going from `1.0` (very cautious) to `0.0` (very eager).
For example, a mildly cautious node can be configured using a configuration snippet like this:

```toml
[identity]
rollout_wariness = 0.5
```

A common case is to have few dedicated nodes, also known as "canaries", that are configured to be very eager to receive updates, with a rollout wariness equal to `0.0`.
Those nodes are meant to receive updates as soon as they are available, can afford some downtime, and are specifically monitored in order to detect issues before they start affecting a larger fleet of machines.

It is recommended to setup and monitor canary nodes, but otherwise normal worker nodes should not have zero wariness.

The default and recommended configuration does not set any static wariness value on Zincati side, leaving rollout decisions to Cincinnati backend.

## Strategies for updates finalization

Zincati actively tries to detect and stage new updates whenever they become available.
Once a new payload has been locally staged, a machine reboot is required in order to atomically apply the update to the system as a whole.

Rebooting a machine does affect any workloads running on the machine at that time, and can potentially impact services across a whole cluster of nodes.
For such reason, Zincati allows the user to control when a node is allowed to reboot to finalize an auto-update.

The following finalization strategies are currently available:
 * immediately reboot to apply an update, as soon as it is downloaded and staged locally (`immediate` strategy, see [relevant documentation][strategy-immediate]).
 * use an external lock-manager to reboot a fleet of machines in a coordinated way (`fleet_lock` strategy, see [relevant documentation][strategy-fleet_lock]).
 * allow reboots only within locally configured maintenance windows, defined on a weekly basis (`periodic` strategy, see [relevant documentation][strategy-periodic]).

By default, the `immediate` strategy is used in order to proactively keep machines up-to-date.

For further documentation on configurations, check the [updates strategy][updates-strategy] documentation.

[strategy-immediate]: updates-strategy.md#immediate-strategy
[strategy-fleet_lock]: updates-strategy.md#lock-based-strategy
[strategy-periodic]: updates-strategy.md#periodic-strategy
[updates-strategy]: updates-strategy.md

## Updates ordering and downgrades

OS updates have a strict ascending ordering called "age index", which is based on the date and time of release.
Versions that have been released earlier in time have a lower index than recent ones.

Zincati uses this absolute ordering to prefer newer releases (i.e. with higher age index) when multiple updates are available at the same time.
By default, this ordering is also used to prevent automatic downgrades.

For custom environments where automatic downgrades have to be supported, the following configuration snippet can be used to enable them:

```toml
[updates]
allow_downgrade = true
```

Enabling such logic removes an additional safety check, and may allow rogue Cincinnati servers to induce downgrades to old releases with known security vulnerabilities.
It is generally not recommended to allow and perform automatic downgrades via Zincati.

## Disabling auto-updates

To disable auto-updates, a configuration snippet containing the following has to be installed on the system:

```toml
[updates]
enabled = false
```

Make sure that it has higher priority than previous settings, by using a path like `/etc/zincati/config.d/90-disable-auto-updates.toml`.

When auto-updates are disabled, Zincati does not perform any update action.
However, the service does not terminate and is kept alive idle for external status observers. 
