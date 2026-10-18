#!/bin/sh
# Browser tests for OpenDM Connect: Google Chrome and Firefox load the
# unpacked extension, which hands downloads to build/OpenDM.app through the
# native host. Run scripts/build-macos.sh first. Needs Chrome, Firefox,
# ffmpeg, Python 3 and Node 22 or later. Uses OPENDM_E2E_DIR (default
# /tmp/opendm-e2e) for the test site, browser profiles and downloads.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
export OPENDM_E2E_DIR="${OPENDM_E2E_DIR:-/tmp/opendm-e2e}"
site="$OPENDM_E2E_DIR/site"
mkdir -p "$site/files" "$site/hls2"
cp "$here"/site/*.html "$site/"
[ -f "$site/files/report.zip" ] || head -c 6291456 /dev/urandom > "$site/files/report.zip"
[ -f "$site/files/clip.webm" ] || ffmpeg -loglevel error -y -f lavfi -i testsrc=size=320x180:rate=25 -t 4 \
    -c:v libvpx -b:v 300k "$site/files/clip.webm"
[ -f "$site/hls2/stream.m3u8" ] || ffmpeg -loglevel error -y -f lavfi -i testsrc=size=640x360:rate=25 \
    -f lavfi -i sine=frequency=440 -t 8 -c:v libx264 -g 50 -keyint_min 50 -sc_threshold 0 -pix_fmt yuv420p \
    -c:a aac -f hls -hls_time 2 -hls_playlist_type vod -hls_segment_filename "$site/hls2/seg%02d.ts" "$site/hls2/stream.m3u8"
: > "$OPENDM_E2E_DIR/requests.log"

python3 "$here/server.py" 8770 &
server=$!
trap 'kill $server' EXIT
sleep 1

# Each test starts OpenDM through the host, so no copy may be running.
quit_app() {
    pkill -TERM -f 'build/OpenDM.app/Contents/MacOS/OpenDM' || true
    sleep 2
}
quit_app
node "$here/chrome.mjs"
quit_app
node "$here/firefox.mjs"
quit_app

