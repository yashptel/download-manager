# Browser test for OpenDM Connect on Windows: Google Chrome (or the browser
# in $env:CHROME, such as Edge) loads the unpacked extension, which hands
# downloads to build\windows\OpenDM-x64 through the native host. Run
# scripts\build-windows.ps1 first. Needs Chrome, ffmpeg, Python 3 and Node 22
# or later. Uses $env:OPENDM_E2E_DIR (default %TEMP%\opendm-e2e) for the test
# site, browser profile and downloads. The test starts OpenDM through the
# host, so it quits a running copy first.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
if (-not $env:OPENDM_E2E_DIR) { $env:OPENDM_E2E_DIR = Join-Path $env:TEMP 'opendm-e2e' }
$site = Join-Path $env:OPENDM_E2E_DIR 'site'
foreach ($dir in 'files', 'hls\360', 'hls\720', 'api') { New-Item -ItemType Directory -Force (Join-Path $site $dir) | Out-Null }
Copy-Item "$here\site\*.html" $site

$zip = Join-Path $site 'files\report.zip'
if (-not (Test-Path $zip)) {
    $bytes = New-Object byte[] 6291456
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    [IO.File]::WriteAllBytes($zip, $bytes)
}
$clip = Join-Path $site 'files\clip.webm'
if (-not (Test-Path $clip)) {
    ffmpeg -loglevel error -y -f lavfi -i testsrc=size=320x180:rate=25 -t 4 -c:v libvpx -b:v 300k $clip
}
# A two-quality HLS stream, the way video sites serve one.
foreach ($height in 360, 720) {
    $folder = Join-Path $site "hls\$height"
    if (-not (Test-Path "$folder\stream.m3u8")) {
        $video = "testsrc=size=$($height * 16 / 9)x$($height):rate=25"
        $hls = @('-c:v', 'libx264', '-g', '50', '-keyint_min', '50', '-sc_threshold', '0', '-pix_fmt', 'yuv420p', '-c:a', 'aac',
            '-f', 'hls', '-hls_time', '2', '-hls_playlist_type', 'vod', '-hls_segment_filename', "$folder\seg%02d.ts")
        ffmpeg -loglevel error -y -f lavfi -i $video -f lavfi -i sine=frequency=440 -t 8 @hls "$folder\stream.m3u8"
    }
}
[IO.File]::WriteAllText((Join-Path $site 'hls\master.m3u8'), (@(
    '#EXTM3U'
    '#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,CODECS="avc1.64001e,mp4a.40.2"'
    '360/stream.m3u8'
    '#EXT-X-STREAM-INF:BANDWIDTH=2400000,RESOLUTION=1280x720,CODECS="avc1.64001f,mp4a.40.2"'
    '720/stream.m3u8'
    ''
) -join "`n"))
# The player's API answer names the playlist only inside escaped JSON.
[IO.File]::WriteAllText((Join-Path $site 'api\video.json'), '{"title":"Big Buck Test Video","sources":{"hls":"http:\/\/127.0.0.1:8770\/hls\/master.m3u8?token=t1"}}')
[IO.File]::WriteAllText((Join-Path $env:OPENDM_E2E_DIR 'requests.log'), '')

Stop-Process -Name OpenDM -ErrorAction SilentlyContinue
Start-Sleep 2
$server = Start-Process python -ArgumentList '-B', "$here\server.py", '8770' -WindowStyle Hidden -PassThru
try {
    Start-Sleep 1
    node "$here\chrome.mjs"
    $failed = $LASTEXITCODE
} finally {
    Stop-Process -Id $server.Id -ErrorAction SilentlyContinue
    Stop-Process -Name OpenDM -ErrorAction SilentlyContinue
}
exit $failed
