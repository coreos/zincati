---
nav_order: 6
parent: Development
---

# Periodic update strategy

The agent supports a `periodic` strategy, which allows gating reboots based on "reboot windows", defined on weekly basis.

This strategy is a port of [locksmith reboot windows][locksmith], with a few differences:

 * multiple disjoint reboot windows are supported
 * multiple configuration entries are assembled into a single weekly calendar
 * weekdays need to be specified, in either long or abbreviated form
 * length duration is always specified in minutes

[locksmith]: https://github.com/coreos/locksmith/tree/v0.6.2#reboot-windows

# Timing and configuration

Window granularity is at the "minutes" level. For this reason, the configuration parameter `length_minutes` is a plain non-zero integer (instead of a free-form duration string).

In order to ease the case where the same time-window has to be applied on multiple specific days, the `days` parameter accepts a set of weekdays (instead of a single day).

The start of a reboot window is a single point in time, specified in 24h format with minutes granularity (e.g. `22:30`) via the `start_time` parameter.

By default, all times and dates are UTC-based.
UTC times must be used to avoid:

 * shortening or skipping reboot windows due to Daylight Saving Time time-change
 * lengthening reboot windows due to Daylight Saving Time time-change
 * mixups due to short-notice law changes in time-zone definitions
 * errors due to stale `tzdata` entries
 * human confusion on machines with different local-timezone configurations

Overall, the use of the default UTC times guarantee that the total weekly length for reboot windows is respected, regardless of local time zone laws.

As a side-effect, this also helps when cross-checking configurations across multiple machines located in different places.

Nevertheless, user-specified non-UTC time zones can still be configured, but with [caveats][time-zone-caveats].

[time-zone-caveats]: ../usage/updates-strategy.md#time-zone-caveats

# Implementation details

Configuration fragments are merged into a single weekly calendar.

In order to avoid too many unwieldy datetime operations to be performed in "modulo 7 days", all times are converted to "minutes since beginning of week".
This means that all datetimes are mapped to the range that goes from `0` (00:00 on Monday morning) to `MAX_WEEKLY_MINS` (23:59 on Sunday night).
A reboot window which is specified across week boundary (e.g. starting on Sunday and ending on Monday) gets split into two sub-windows in order to respect the range above.

Reboot windows are internally stored within an [Augmented Interval Tree](https://en.wikipedia.org/wiki/Interval_tree#Augmented_tree) data-structure.
