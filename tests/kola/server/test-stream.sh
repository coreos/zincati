#!/bin/bash     

# Test to check for correct detection of incorrect stream metadata in new releases and denylist functionality.

set -xeuo pipefail

. ${KOLA_EXT_DATA}/libtest.sh

wait_journal_has_content() {
  regex=$1

  for i in {1..24}
  do
    journalctl -u zincati.service > journal.txt
    if grep -q "$regex" journal.txt; then
      break
    fi
    sleep 5
  done
  journalctl -u zincati.service > journal.txt
  assert_file_has_content journal.txt "$regex"
}

cd $(mktemp -d)

case "${AUTOPKGTEST_REBOOT_MARK:-}" in
  "")
    # Prepare a graph template with two nodes. The node with the lower age index will be
    # populated with the current booted deployment, and the node with the higher age index
    # will be populated with a new version to possibly update to.
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
    # Create a new commit on `test-branch` that has an unknown stream.
    bad_version="$cur_version".bad-update
    bad_payload="$(ostree --repo=/var/www commit --branch=test-branch --tree ref="$cur_payload" \
                    --add-metadata-string version="$bad_version" --add-metadata-string fedora-coreos.stream='unknown-stream' \
                    --keep-metadata='coreos-assembler.basearch' --parent="$cur_payload")"

    jq \
        --arg cur_version "$cur_version" \
        --arg cur_payload "$cur_payload" \
        --arg bad_version "$bad_version" \
        --arg bad_payload "$bad_payload" \
        '.nodes[0].version=$cur_version | .nodes[0].payload=$cur_payload | .nodes[1].version=$bad_version | .nodes[1].payload=$bad_payload' \
        graph_template > /var/www/v1/graph

    # Now let Zincati check for updates.
    echo "updates.enabled = true" > /etc/zincati/config.d/99-test-status-updates-enabled.toml
    systemctl restart zincati.service

    wait_journal_has_content "deployed an update on different update stream, abandoning update ${bad_version}"
    ok "abandon update on different stream"

    wait_journal_has_content "Found 1 possible update target present in denylist; ignoring"
    ok "abandoned updates with incorrect stream in denylist"

    systemctl stop zincati.service

    # Create a new commit on `test-branch` that's on the correct stream.
    good_version="$cur_version".good-update
    good_payload="$(ostree --repo=/var/www commit --branch=test-branch --tree ref="$cur_payload" \
                    --add-metadata-string version="$good_version" --keep-metadata='fedora-coreos.stream' \
                    --keep-metadata='coreos-assembler.basearch' --parent="$cur_payload")"

    jq \
        --arg cur_version "$cur_version" \
        --arg cur_payload "$cur_payload" \
        --arg good_version "$good_version" \
        --arg good_payload "$good_payload" \
        '.nodes[0].version=$cur_version | .nodes[0].payload=$cur_payload | .nodes[1].version=$good_version | .nodes[1].payload=$good_payload' \
        graph_template > /var/www/v1/graph

    # We need to rebase onto our local OSTree repo's `test-branch` again because Zincati will tell rpm-ostree to cleanup bad releases.
    rpm-ostree rebase local:test-branch

    /tmp/autopkgtest-reboot-prepare rebooted
    systemctl start zincati.service
    ;;

  rebooted)
    rpm-ostree status > status.txt
    assert_file_has_content status.txt "Version:.*good-update"
    ok "succesfully stage update on same stream"
    ;;

  *) echo "unexpected mark: ${AUTOPKGTEST_REBOOT_MARK}"; exit 1;;
esac
