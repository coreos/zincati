---
layout: default
parent: Usage
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

This protocol is not coupled to any specific backend, and can be implemented on top of any suitable infrastructure:
 * [airlock] is a free-software project which implements such protocol on top of [etcd3].
 * a Kubernetes-based reboot-manager is provided as part of [Typhoon](https://github.com/poseidon/fleetlock).
 * <https://github.com/opencounter/terraform-fleet-lock-dynamodb> is a serverless implementation via AWS API Gateway and DynamoDB.

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

[fleet_lock]: ../development/fleetlock/protocol.md
[airlock]: https://github.com/coreos/airlock
[etcd3]: https://etcd.io/

# File-based strategy
The `marker_file` strategy is a simple, low-level strategy that only allows Zincati to reboot for updates when a specific marker file exists on the local filesystem.

Similar to the `fleet_lock` strategy, the `marker_file` strategy provides a large amount of flexibility to admins, and should be used with a central controller.
Unlike `fleet_lock`, where the central controller must be a lock-manager on the network, the central controller for the `marker_file` strategy can be a containerized agent, some central task manager able to manipulate files on machines (e.g. Ansible), or even a human via SSH.

To indicate that a machine is allowed to finalize an update and reboot, a file with the following properties must be present on the machine's local filesystem:
    - named `allowfinalize.json`
    - under `/var/lib/zincati/admin/strategy/marker_file`
    - is a valid JSON file
    - not writable by others

If any of the above is not satisfied in your marker file, Zincati will not allow reboots.

`allowfinalize.json` can optionally contain an `allowUntil` key with a Unix timestamp integer as its value to indicate the expiry date and time of this marker file. If the current time timestamp is _greater than or equal to_ this timestamp, then reboots will not be allowed.
Otherwise, if the `allowUntil` key is not present, reboots will be allowed for as long as `allowfinalize.json` exists (in the right location), and it must be removed to disallow reboots.
Note that `allowfinalize.json` must still be a valid JSON file, regardless of whether the `allowUntil` key is present.

For example, if you wish to allow reboots until the end of April 2021 UTC, create a JSON file with path `/var/lib/zincati/admin/strategy/marker_file/allowfinalize.json` (Unix timestamp 1619827200 is May 01 2021 00:00:00 UTC):

```json
{
    "allowUntil": 1619827200
}
```

The above JSON file can be created using `jq` by entering the following command:

```bash
echo '"2021-05-01T00:00:00Z"' | jq '{allowUntil: 'fromdateiso8601'}' \
| sudo tee /var/lib/zincati/admin/strategy/marker_file/allowfinalize.json
```

Warning: In `jq` versions `1.6` and lower, `jq` [may output incorrect Unix timestamps][jq_bug] for certain datetimes on machines with certain `localtime`s.

If you wish to allow reboots for as long as the marker file is present, create an empty JSON file with path `/var/lib/zincati/admin/strategy/marker_file/allowfinalize.json`:

```json
{}
```

An empty JSON file can be created by entering:

```bash
echo '{}' | sudo tee /var/lib/zincati/admin/strategy/marker_file/allowfinalize.json
```

For configuration purposes, such strategy is labeled `marker_file` and takes no additional configuration parameters.

This strategy can be enabled via a configuration snippet like the following:

```toml
[updates]
strategy = "marker_file"
```

[jq_bug]: https://github.com/stedolan/jq/issues/2001

# Periodic strategy

The `periodic` strategy allows Zincati to only reboot for updates during certain timeframes, also known as "maintenance windows" or "reboot windows".
Outside of those maintenance windows, reboots are not automatically performed and auto-updates are staged and held until the next available window.

Reboot windows recur on a weekly basis, and can be defined in any arbitrary order and length. Their individual length must be greater than zero.
By default, all maintenance windows are defined in UTC dates and times. This is meant to avoid timezone-related skews in a fleet of machines, as well as possible side-effects of Daylight Savings Time (DST) policies.

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

## Time zone configuration

To configure a non-UTC time zone for all the reboot windows, specify the `time_zone` field in a `updates.periodic` entry. The specified time zone must be either `"localtime"` or a time zone name from the [IANA Time Zone Database][IANA_tz_db] (you can find an unofficial list of time zone names [here][wikipedia_tz_names]).

If using `"localtime"`, the system's [local time zone configuration file][localtime], `/etc/localtime`, is used. As such, `/etc/localtime` must either be a symlink to a valid `tzfile` entry in your system's local time zone database (under `/usr/share/zoneinfo/`), or not exist, in which case `UTC` is used.

Note that you can only specify a single time zone for _all_ reboot windows.

A time zone can be specified in the following way:

```toml
[updates]
strategy = "periodic"

[updates.periodic]
time_zone = "America/Panama"

[[updates.periodic.window]]
days = [ "Sat", "Sun" ]
start_time = "23:30"
length_minutes = 60

[[updates.periodic.window]]
days = [ "Mon" ]
start_time = "00:00"
length_minutes = 60
```

Since Panama does not have Daylight Savings Time and follows Eastern Standard Time (which has a fixed offset of UTC -5) all year, the above configuration would result in two maintenance windows during which Zincati is allowed to reboot the machine for updates:
 * 60 minutes starting at 23:30 EST on Saturday night, and ending at 00:30 EST on Sunday morning
 * 90 minutes starting at 23:30 EST on Sunday night, and ending at 01:00 EST on Monday morning

### Time zone caveats

⚠️ **Reboot window lengths may vary.** ⚠️

Because reboot window clock times are always obeyed, reboot windows may be lengthened or shortened due to shifts in clock time. For example, with the `US/Eastern` time zone which shifts between Eastern Standard Time and Eastern Daylight Time, on "fall back" day, a specified reboot window may be lengthened by up to one hour; on "spring forward" day, a specified reboot window may be shortened by up to one hour, or skipped entirely.

Example of varying length reboot windows using the `US/Eastern` time zone:

```toml
[updates]
strategy = "periodic"

[updates.periodic]
time_zone = "US/Eastern"

[[updates.periodic.window]]
days = [ "Sun" ]
start_time = "01:30"
length_minutes = 60
```

The above configuration will result in reboots being allowed at 1:30 AM to 2:30 AM on _every_ Sunday. This includes days when a Daylight Savings Shift occurs.

On the `US/Eastern` time zone's "fall back" day, where clocks are shifted back by one hour on a Sunday in Fall just before 3:00 AM, the thirty minutes between 2:00 AM and 2:30 AM will occur twice. As such, the reboot window will be lengthened by thirty minutes each year on "fall back" day.

On "spring forward" day, where clocks are shifted forward by one hour on a Sunday in Spring just before 2:00 AM, the thirty minutes between 2:00 AM and 2:30 AM will not occur. As such, the reboot window will be shortened by thirty minutes each year on "spring forward" day. Effectively, the reboot window on "spring forward" day will only be between 1:30 AM and 2:00 AM.

⚠️ **Incorrect reboot times due to stale time zone database.** ⚠️

Time zone data is read from the system's time zone database at `/usr/share/zoneinfo`. This directory and its contents are part of the `tzdata` RPM package; in the latest release of Fedora CoreOS, `tzdata` should be kept fairly up-to-date with the latest official release from the IANA.
However, if your system does not have the latest IANA time zone database, or there is a sudden policy change in the jurisdiction associated with your configured time zone, then reboots may happen at unexpected and incorrect times.

[IANA_tz_db]: https://www.iana.org/time-zones
[wikipedia_tz_names]: https://en.wikipedia.org/wiki/List_of_tz_database_time_zones
[localtime]: https://www.freedesktop.org/software/systemd/man/localtime.html
