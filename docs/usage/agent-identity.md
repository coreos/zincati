---
layout: default
parent: Usage
---

# Agent identity

Zincati agent tries to derive a unique identity for the machine it is running on by introspecting the underlying OS and reading user configuration.
This includes assigning an ID and a group label specific to the agent, so that cluster-wide upgrades can be orchestrated via [phased rollouts][phased] and [lock-based][fleetlock-strategy] reboots.

[phased]: auto-updates.md#phased-rollouts-client-wariness-canaries
[fleetlock-strategy]:  updates-strategy.md#lock-based-strategy

## Identity configuration

All agent identity values are normally auto-detected at startup and do not require user intervention.

However, the following settings can be overridden through configuration fragments in the `identity` section:
 * `group`: group label, used for graph fetching ([Cincinnati][cincinnati]) and reboot orchestration ([FleetLock][fleetlock])
 * `node_uuid`: agent ID, used for graph fetching ([Cincinnati][cincinnati]) and reboot orchestration ([FleetLock][fleetlock])
 * `rollout_wariness`: agent wariness to [phased rollouts][phased], used for graph fetching ([Cincinnati][cincinnati]).

The following are defaults for each setting:
- `group` (group label) is set to `default`
- `node_uuid` (agent ID) is automatically generated, by hashing `/etc/machine-id` content
- `rollout_wariness` is unset and the Cincinnati backend will assign a dynamic value to each request

When the agent ID is not customized via configuration fragments, its default value is dynamically generated starting from `/etc/machine-id` content and from a Zincati specific application ID.
For more details about such application-specific machine IDs, see [machine-id][machine-id] documentation.

[machine-id]: https://www.freedesktop.org/software/systemd/man/machine-id.html

## Example

As an example, users can specify custom identity parameters by writing a configuration fragment to `/etc/lib/zincati/config.d/90-custom-identity.toml`:

```toml
[identity]
group = "workers"
```

The fragment above will steer the node into the "workers" reboot group.

[cincinnati]: ../development/cincinnati/protocol.md
[fleetlock]: ../development/fleetlock/protocol.md
