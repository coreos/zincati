#!/bin/bash     

# Simple sanity checks for systemd unit status updates.

set -xeuo pipefail

. ${KOLA_EXT_DATA}/libtest.sh

cd $(mktemp -d)

systemctl stop zincati.service
echo "updates.enabled = false" > /etc/zincati/config.d/99-test-status-updates-enabled.toml
systemctl start zincati.service
systemctl show -p StatusText zincati.service > zincati_disabled_status.txt
assert_file_has_content zincati_disabled_status.txt \
'initialization complete, auto-updates logic disabled by configuration'
ok "status show initialization"

systemctl stop zincati.service
echo "updates.enabled = true" > /etc/zincati/config.d/99-test-status-updates-enabled.toml
systemctl start zincati.service
systemctl show -p StatusText zincati.service > zincati_polling_status.txt
assert_file_has_content zincati_polling_status.txt 'periodically polling for updates'
ok "status show polling"

# Wait for Zincati to check for updates.
for i in {1..30}
do
    systemctl show -p StatusText zincati.service > zincati_last_check_status.txt
    if grep -q 'last checked' zincati_last_check_status.txt; then
        break
    fi
    sleep 1
done
echo "timed out waiting for Zincati to check for updates"
assert_file_has_content zincati_last_check_status.txt 'periodically polling for updates (last checked'
ok "status show last check"
