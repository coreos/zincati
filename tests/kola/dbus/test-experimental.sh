#!/bin/bash     

# Tests for the `org.coreos.zincati1.Experimental` interface.

set -xeuo pipefail

. ${KOLA_EXT_DATA}/libtest.sh

cd $(mktemp -d)

# Ensure that methods in this interface can only be called by root.
if sudo -u core busctl call org.coreos.zincati1 /org/coreos/zincati1 org.coreos.zincati1.Experimental LastRefresh 2> err.txt; then
  fatal "Non-root user calling Experimental interface unexpectedly succeeded"
fi
assert_file_has_content err.txt "Access denied"
ok "only allow root to call Experimental interface"

# Check Moo method.
busctl call org.coreos.zincati1 /org/coreos/zincati1 org.coreos.zincati1.Experimental Moo b yes > output.txt
assert_file_has_content output.txt "Moooo mooo moooo!"
busctl call org.coreos.zincati1 /org/coreos/zincati1 org.coreos.zincati1.Experimental Moo b no > output.txt
assert_file_has_content output.txt "moo."
ok "Moo method"

# Check LastRefreshTime method.
# First, get the last refresh time
response=$(busctl call org.coreos.zincati1 /org/coreos/zincati1 org.coreos.zincati1.Experimental LastRefreshTime)
last_refresh_time=$(echo "${response}" | sed 's/[^0-9]*//g')
# Sanity check that the last refresh time is a reasonable time.
test "${last_refresh_time}" -gt 1616414400 # 1616414400 is Monday, March 22, 2021 12:00:00 PM UTC.
ok "LastRefreshTime method"

# Check that CLI commands work.
/usr/libexec/zincati ex moo --talkative > output.txt
assert_file_has_content output.txt "Moooo mooo moooo!"
test $(/usr/libexec/zincati ex last-refresh-time) -gt 1616414400
ok "last-refresh-time CLI command"
