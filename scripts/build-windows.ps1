# Builds the Windows app into build\windows\OpenDM-<arch>: the Rust core, its
# C# bindings, the WinUI 3 app and the pinned aria2-next engine.
#   scripts\build-windows.ps1                      # x64, Release
#   scripts\build-windows.ps1 -Arch arm64
#   scripts\build-windows.ps1 -Configuration Debug
param(
    [ValidateSet("x64", "arm64")]
    [string]$Arch = "x64",
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Invoke-Native([string]$File, [string[]]$Arguments) {
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$File $($Arguments -join ' ') failed with exit code $LASTEXITCODE" }
}

$triple = @{ x64 = "x86_64-pc-windows-msvc"; arm64 = "aarch64-pc-windows-msvc" }[$Arch]
$enginePlatform = @{ x64 = "windows-x86_64.exe"; arm64 = "windows-arm64.exe" }[$Arch]
$engine = & (Join-Path $PSScriptRoot "fetch-engine.ps1") -Platform $enginePlatform

Invoke-Native rustup @("target", "add", $triple)
Invoke-Native cargo @("build", "--release", "-p", "opendm-core", "-p", "opendm-host", "--target", $triple)
$library = Join-Path $root "target\$triple\release\opendm_core.dll"
$hostExe = Join-Path $root "target\$triple\release\opendm-host.exe"

# C# bindings from the library's UniFFI metadata. The generator version must
# match the uniffi version in core\Cargo.toml (0.31).
$tools = Join-Path $root "build\tools"
$bindgen = Join-Path $tools "bin\uniffi-bindgen-cs.exe"
if (-not (Test-Path $bindgen)) {
    Invoke-Native cargo @("install", "uniffi-bindgen-cs", "--git", "https://github.com/NordSecurity/uniffi-bindgen-cs",
        "--tag", "v0.11.1+v0.31.0", "--root", $tools)
}
$app = Join-Path $root "windows\OpenDM"
Invoke-Native $bindgen @("--library", $library, "--out-dir", (Join-Path $app "Core"), "--no-format")

# Native files the project copies next to OpenDM.exe.
$native = Join-Path $app "Native"
if (Test-Path $native) { Remove-Item -Recurse -Force $native }
New-Item -ItemType Directory -Force -Path $native | Out-Null
Copy-Item $library $native
Copy-Item $hostExe $native
Copy-Item $engine (Join-Path $native "aria2-next.exe")
Copy-Item (Join-Path $root "vendor\aria2-next\bin\COPYING") (Join-Path $native "aria2-next-COPYING.txt")
Copy-Item (Join-Path $root "LICENSE") (Join-Path $native "LICENSE.txt")

$out = Join-Path $root "build\windows\OpenDM-$Arch"
Invoke-Native dotnet @("publish", (Join-Path $app "OpenDM.csproj"), "-c", $Configuration, "-r", "win-$Arch",
    "-p:Platform=$Arch", "-o", $out)
Write-Host (Join-Path $out "OpenDM.exe")
