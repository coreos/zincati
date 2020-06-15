local grafana = import 'github.com/grafana/grafonnet-lib/grafonnet/grafana.libsonnet';
local dashboard = grafana.dashboard;
local row = grafana.row;
local prometheus = grafana.prometheus;
local graphPanel = grafana.graphPanel;

{
  grafanaDashboards+:: {
    'dashboard.json':
      dashboard.new(
        'Fedora CoreOS updates (Zincati)',
        time_from='now-7d',
      ).addTemplate(
        {
          current: {
            text: 'Prometheus',
            value: 'Prometheus',
          },
          hide: 0,
          label: null,
          name: 'datasource',
          options: [],
          query: 'prometheus',
          refresh: 1,
          regex: '',
          type: 'datasource',
        },
      )
      .addRow(
        row.new(
          title='Agent identity',
        )
        .addPanel(
          graphPanel.new(
            'OS versions',
            datasource='$datasource',
            decimalsY1=0,
            format='short',
            legend_alignAsTable=true,
            legend_current=true,
            legend_show=true,
            legend_values=true,
            min=0,
            span=6,
            stack=true,
          )
          .addTarget(prometheus.target(
            'sum by(os_version) (zincati_identity_os_info)',
            legendFormat='{{os_version}}'
          ))
        )
        .addPanel(
          graphPanel.new(
            'Static rollout wariness',
            datasource='$datasource',
            format='short',
            legend_show=true,
            min=0,
            span=6,
          )
          .addTarget(prometheus.target(
            'zincati_identity_rollout_wariness != 0',
            legendFormat='{{instance}}'
          ))
        )
      )
      .addRow(
        row.new(
          title='Agent details',
        )
        .addPanel(
          graphPanel.new(
            'Agent refresh period (p99)',
            datasource='$datasource',
            formatY1='s',
            span=6,
            min=0,
          )
          .addTarget(prometheus.target(
            'quantile_over_time(0.99, (time() - zincati_update_agent_last_refresh_timestamp)[15m:])',
            legendFormat='{{instance}}'
          ))
        )
        .addPanel(
          graphPanel.new(
            'Cincinnati client error-rate',
            datasource='$datasource',
            span=6,
            min=0,
          )
          .addTarget(prometheus.target(
            'sum by (kind) (rate(zincati_cincinnati_update_checks_errors_total[5m]))',
            legendFormat='kind: {{kind}}'
          ))
        )
        .addPanel(
          graphPanel.new(
            'Deadends detected',
            datasource='$datasource',
            decimalsY1=0,
            format='short',
            legend_alignAsTable=true,
            legend_current=true,
            legend_show=true,
            legend_values=true,
            min=0,
            span=6,
            stack=true,
          )
          .addTarget(prometheus.target(
            'sum by (os_version) ((zincati_cincinnati_booted_release_is_deadend) + on (instance) group_left(os_version) (0*zincati_identity_os_info))',
            legendFormat='{{os_version}}'
          ))
        )
      ),
  },
}
