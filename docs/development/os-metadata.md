---
title: OS metadata and agent identity
parent: Development
nav_order: 1
layout: default
---

# OS metadata and agent identity

The agent needs to derive its own identity from several aspects of the underlying OS.
In order to do so, at startup it performs run-time introspection of current machine state and OS metadata.

The following details are derived from the host environment:

 * application-specific node UUID
 * base architecture
 * update stream
 * OS platform
 * OS version
 * OSTree revision

It is thus required that the OS provides those values in the locations described below.

### Kernel command-line

Kernel command-line must contain a `ignition.platform.id=<VALUE>` argument. The literal value is used as the "OS platform".

### rpm-ostree deployment status

Booted deployment must provide several mandatory metadata entries:

 * `checksum`: OSTree commit revision
 * `version`: OS version
 * under `base-commit-meta`:
   * `coreos-assembler.basearch`: base architecture
   * `fedora-coreos.stream`: update stream

All those metadata entries must exist with a non-empty string value.

### Filesystem

Filesystem must provide a `/etc/machine-id` file, as specified by [machine-id spec][machine-id]. Its value is used to derive the application-specific node UUID.

[machine-id]: https://www.freedesktop.org/software/systemd/man/machine-id.html
