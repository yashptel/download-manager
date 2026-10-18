# Downloads the pinned aria2-next build for Windows and verifies its SHA-256.
# Prints the path of the engine executable.
#   scripts\fetch-engine.ps1                                 # x64
#   scripts\fetch-engine.ps1 -Platform windows-arm64.exe
param(
    [ValidateSet("windows-x86_64.exe", "windows-arm64.exe")]
    [string]$Platform = "windows-x86_64.exe"
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$root = Split-Path -Parent $PSScriptRoot
$vendor = Join-Path $root "vendor\aria2-next"
$version = (Get-Content (Join-Path $vendor "VERSION") -Raw).Trim()
$asset = "aria2-next-$version-$Platform"

$line = Get-Content (Join-Path $vendor "checksums.sha256") |
    Where-Object { ($_ -split "\s+")[1] -eq $asset } |
    Select-Object -First 1
if (-not $line) { throw "No checksum for $asset in vendor\aria2-next\checksums.sha256" }
$expected = ($line -split "\s+")[0]

$bin = Join-Path $vendor "bin"
New-Item -ItemType Directory -Force -Path $bin | Out-Null
$target = Join-Path $bin "aria2-next-$Platform"
if ((Test-Path $target) -and (Get-FileHash $target -Algorithm SHA256).Hash -eq $expected) {
    return $target
}

$part = "$target.part"
Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/AnInsomniacy/aria2-next/releases/download/v$version/$asset" -OutFile $part
$actual = (Get-FileHash $part -Algorithm SHA256).Hash
if ($actual -ne $expected) {
    Remove-Item $part
    throw "Checksum mismatch for ${asset}: expected $expected, got $actual"
}
Move-Item -Force $part $target
Invoke-WebRequest -UseBasicParsing -Uri "https://raw.githubusercontent.com/AnInsomniacy/aria2-next/v$version/COPYING" -OutFile (Join-Path $bin "COPYING")
return $target
