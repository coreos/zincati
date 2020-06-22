# Requirements

In order to customize and generate monitoring artifacts, the following tools are required:

 * `jb` available at <https://github.com/jsonnet-bundler/jsonnet-bundler>.
 * `jsonnet` available at <https://github.com/google/jsonnet>.
 * `mixtool` available at <https://github.com/monitoring-mixins/mixtool>.

For more information, see <https://monitoring.mixins.dev/>.

# Artifacts generation

Monitoring artifacts can be generated from mixins in a few steps:

```sh
# Clean stale artifacts.
rm -rf vendor/ generated/ jsonnetfile.lock.json

# Fetch jsonnet libraries.
jb install

# Generate Grafana dashboards.
mixtool generate dashboards -d generated/dashboards/ mixin.libsonnet
```
