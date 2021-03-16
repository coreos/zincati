#!/bin/bash     

# Test to check for correct detection of a dead-end release.

set -xeuo pipefail

cd $(mktemp -d)

# Prepare a graph with the current booted deployment as a dead-end.
mkdir /var/www/v1
cat <<'EOF' > graph_template
{
  "nodes": [
    {
      "version": "",
      "metadata": {
          "org.fedoraproject.coreos.releases.age_index": "0",
          "org.fedoraproject.coreos.updates.deadend_reason": "https://github.com/coreos/fedora-coreos-tracker/issues/215",
          "org.fedoraproject.coreos.scheme": "checksum",
          "org.fedoraproject.coreos.updates.deadend": "true"
      },
      "payload": ""
    }
  ],
  "edges": []
}
EOF
version="$(/usr/bin/rpm-ostree status --json | jq '.deployments[0].version' -r)"
payload="$(/usr/bin/rpm-ostree status --json | jq '.deployments[0].checksum' -r)"
jq \
  --arg version "$version" \
  --arg payload "$payload" \
  '.nodes[0].version=$version | .nodes[0].payload=$payload' \
  graph_template >/var/www/v1/graph


# Now let Zincati check for updates (and detect that the current release is a dead-end).
echo "updates.enabled = true" > /etc/zincati/config.d/99-test-status-updates-enabled.toml
systemctl restart zincati.service

# Wait up to 60 seconds for Zincati to detect that the current release is a dead-end release.
for i in {1..60}
do
    if test -f /run/motd.d/85-zincati-deadend.motd; then
        exit 0
    fi
    sleep 1
done

echo "Dead-end MOTD file not found after timeout."
exit 1
