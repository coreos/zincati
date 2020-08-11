---
title: Configuration
parent: Usage
layout: default
---

# Configuration

Zincati supports runtime customization via configuration fragments (dropins), allowing users and distributions to tweak the agent behavior by writing plain-text files.

Each configuration fragment is a TOML snippet which is read by Zincati and assembled into the final runtime configuration. Only files with a `.toml` extension are considered.

Dropins are sourced from multiple directories, and merged by filename in lexicographic order.

The following configuration paths are scanned by Zincati, in order:
 * `/usr/lib/zincati/config.d/`: distribution defaults, read-only path owned by the OS.
 * `/etc/zincati/config.d/`: user customizations, writable path owned by the system administrator.
 * `/run/zincati/config.d/`: runtime customizations, writable path that is not persisted across reboots.

Configuration directives from files that appear later in sorting order can override prior directives.

If multiple files with the same name exist, only the last-sorting one is read.

Additionally, symbolic links to `/dev/null` can be used to completely override a prior file with the same name.

Configuration dropins are organized in multiple TOML sections, which are described in details in their own documentation pages.

## Example

As an example, distribution defaults may generically enable a feature, but users may need to disable that in specific case.

To that extent, distributions can provide by default the following content at `/usr/lib/zincati/config.d/10-enable-feature.toml`:

```toml
[feature]
enabled = true
```

In order to override that setting, users can write the following to `/etc/zincati/config.d/90-disable-feature.toml`:

```toml
[feature]
enabled = false
```

After sorting all configuration directives by directory and filename priority, the user-provided dropin is considered with the highest priority. Thus, it will override any conflicting directives from other fragments.
