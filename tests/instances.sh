#!/bin/sh
# Launching OpenDM twice on one data folder leaves one instance running: the
# second hands over to the first and quits. Usage: tests/instances.sh [App.app]
set -eu
app=${1:-build/OpenDM.app}
binary="$app/Contents/MacOS/OpenDM"
data=$(mktemp -d)
OPENDM_DATA_DIR="$data" "$binary" --background >/dev/null 2>&1 &
first=$!
sleep 3
OPENDM_DATA_DIR="$data" "$binary" >/dev/null 2>&1 &
second=$!
sleep 4
alive=0
for pid in $first $second; do kill -0 "$pid" 2>/dev/null && alive=$((alive + 1)); done
sockets=$(ls "$data"/browser.sock 2>/dev/null | wc -l | tr -d ' ')
kill -TERM $first $second 2>/dev/null || true
if [ "$alive" = 1 ] && [ "$sockets" = 1 ]; then
    echo "PASS one instance owns the browser socket (second quit)"
else
    echo "FAIL $alive instances alive after a second launch, $sockets socket"
    exit 1
fi

