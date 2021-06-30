#!/bin/bash     

# Test to check for correct detection of a dead-end release.

set -xeuo pipefail

. ${KOLA_EXT_DATA}/libtest.sh

wait_rpm_ostree_status_has_content() {
  regex=$1

  for i in {1..24}
  do
    rpm-ostree status > status.txt
    if grep -q "$regex" status.txt; then
      break
    fi
    sleep 5
  done
  rpm-ostree status > status.txt
  assert_file_has_content status.txt "$regex"
}

cd $(mktemp -d)

case "${AUTOPKGTEST_REBOOT_MARK:-}" in
  "")
    # Prepare a graph template with two nodes. The node with the lower age index will be
    # populated with the current booted deployment, and the node with the higher age index
    # will be populated with a new version we want to update to.
    mkdir /var/www/v1
    cat <<'EOF' > graph_template
{
  "nodes": [
    {
      "version": "",
      "metadata": {
          "org.fedoraproject.coreos.releases.age_index": "0",
          "org.fedoraproject.coreos.scheme": "checksum"
      },
      "payload": ""
    },
    {
      "version": "",
      "metadata": {
          "org.fedoraproject.coreos.releases.age_index" : "1",
          "org.fedoraproject.coreos.scheme": "checksum"
      },
      "payload": ""
    }
  ],
  "edges": [
    [
      0,
      1
    ]
  ]
}
EOF

    cur_version="$(/usr/bin/rpm-ostree status --json | jq '.deployments[0].version' -r)"
    cur_payload="$(/usr/bin/rpm-ostree status --json | jq '.deployments[0].checksum' -r)"

    # Prepare an OSTree repo in archive mode at `/var/www` and pull the currently booted commit into it.
    ostree --repo=/var/www init --mode="archive"
    ostree --repo=/var/www pull-local /ostree/repo "$cur_payload"
    # Create a new branch `test-branch` by creating a dummy commit.
    ostree --repo=/var/www commit --branch='test-branch' --tree ref="$cur_payload" \
          --add-metadata-string version='dummy' --keep-metadata='fedora-coreos.stream' \
          --keep-metadata='coreos-assembler.basearch' --parent="$cur_payload"
    # Add the OSTree repo at /var/www as a new `local` remote.
    ostree remote add --no-gpg-verify local http://localhost test-branch
    # Rebase onto our local OSTree repo's `test-branch`.
    rpm-ostree rebase local:test-branch
    # Create a new commit on `test-branch`.
    next_version="$cur_version".new-update
    next_payload="$(ostree --repo=/var/www commit --branch=test-branch --tree ref="$cur_payload" \
                    --add-metadata-string version="$next_version" --keep-metadata='fedora-coreos.stream' \
                    --keep-metadata='coreos-assembler.basearch' --parent="$cur_payload")"

    jq \
      --arg cur_version "$cur_version" \
      --arg cur_payload "$cur_payload" \
      --arg next_version "$next_version" \
      --arg next_payload "$next_payload" \
      '.nodes[0].version=$cur_version | .nodes[0].payload=$cur_payload | .nodes[1].version=$next_version | .nodes[1].payload=$next_payload' \
      graph_template > /var/www/v1/graph

    # Set strategy to `marker-file` strategy.
    echo 'updates.strategy = "marker_file"' > /etc/zincati/config.d/95-marker-file-strategy.toml

    # Now let Zincati check for updates (and detect that there is a new release).
    echo "updates.enabled = true" > /etc/zincati/config.d/99-test-status-updates-enabled.toml
    systemctl restart zincati.service

    # Check that Zincati's status is active and stuck at "reboot pending due to update strategy".
    wait_rpm_ostree_status_has_content "active;.*reboot pending due to update strategy"
    ok "disallow reboot when no marker file"

    systemctl stop zincati.service

    # Place marker file with an expired timestamp.
    echo '"2021-05-01T00:00:00Z"' | jq '{allowUntil: 'fromdate'}' \
      > /var/lib/zincati/admin/strategy/marker_file/allowfinalize.json

    systemctl start zincati.service

    # Check that Zincati's status is active and stuck at "reboot pending due to update strategy".
    wait_rpm_ostree_status_has_content "active;.*reboot pending due to update strategy"
    ok "disallow reboot with expired marker file"

    systemctl stop zincati.service

    # Place marker file with no expiry to allow update finalization.
    echo '{}' > /var/lib/zincati/admin/strategy/marker_file/allowfinalize.json

    /tmp/autopkgtest-reboot-prepare rebooted

    systemctl start zincati.service
    ;;

  rebooted)
    rpm-ostree status > status.txt
    assert_file_has_content status.txt "Version:.*new-update"
    ok "allow reboot with non-expired marker file"
    ;;

  *) echo "unexpected mark: ${AUTOPKGTEST_REBOOT_MARK}"; exit 1;;
esac
