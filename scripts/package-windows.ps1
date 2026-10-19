# Builds the Windows installer: build\windows\OpenDM-<version>-<arch>-setup.exe.
# Builds the app first (scripts\build-windows.ps1), then signs it and the
# installer when a code-signing certificate is configured:
#   $env:OPENDM_SIGN_CERT       thumbprint of a certificate in Cert:\CurrentUser\My
#   $env:OPENDM_SIGN_TIMESTAMP  RFC 3161 timestamp server (default DigiCert's)
# Needs Inno Setup 6 (winget install JRSoftware.InnoSetup --scope user).
param(
    [ValidateSet("x64", "arm64")]
    [string]$Arch = "x64"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Invoke-Native([string]$File, [string[]]$Arguments) {
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$File $($Arguments -join ' ') failed with exit code $LASTEXITCODE" }
}

& (Join-Path $PSScriptRoot "build-windows.ps1") -Arch $Arch | Out-Host
$app = Join-Path $root "build\windows\OpenDM-$Arch"
$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

$signtool = $null
if ($env:OPENDM_SIGN_CERT) {
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
        Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
    if (-not $signtool) { throw "signtool.exe not found; install the Windows SDK" }
    $timestamp = if ($env:OPENDM_SIGN_TIMESTAMP) { $env:OPENDM_SIGN_TIMESTAMP } else { "http://timestamp.digicert.com" }
    $signArgs = @("sign", "/sha1", $env:OPENDM_SIGN_CERT, "/s", "My", "/fd", "SHA256", "/tr", $timestamp, "/td", "SHA256")
    $binaries = "OpenDM.exe", "opendm-host.exe", "opendm_core.dll", "aria2-next.exe" | ForEach-Object { Join-Path $app $_ }
    Invoke-Native $signtool ($signArgs + $binaries)
}

$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 not found: winget install JRSoftware.InnoSetup --scope user" }
$out = Join-Path $root "build\windows"
$isccArgs = @("/Q", "/DAppVersion=$version", "/DArch=$Arch", "/DSourceDir=$app", "/DOutputDir=$out")
if ($signtool) {
    # Inno runs this for setup and its uninstaller; $q is a quote, $f the file to sign.
    $isccArgs += @("/DSign", ('/Sopendm=$q' + $signtool + '$q ' + ($signArgs -join " ") + ' $f'))
}
Invoke-Native $iscc ($isccArgs + @((Join-Path $root "windows\installer\OpenDM.iss")))
$setup = Join-Path $out "OpenDM-$version-$Arch-setup.exe"
Write-Host $setup

