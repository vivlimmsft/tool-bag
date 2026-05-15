#requires -Version 5.1
<#
.SYNOPSIS
  Build the webp-tray release binary and a per-user MSI installer,
  optionally signed.

.DESCRIPTION
  Steps:
    1. cargo build --release
    2. sign the .exe (skipped with -SkipSign)
    3. dotnet tool restore (pulls WiX 6)
    4. wix build installer/installer.wxs
    5. sign the .msi (skipped with -SkipSign)

  Signing semantics:
    * Default: signing is REQUIRED. The cert is resolved by sign\sign.ps1
      from -Thumbprint, WEBP_TRAY_SIGN_THUMBPRINT, or WEBP_TRAY_SIGN_PFX +
      WEBP_TRAY_SIGN_PFX_PASSWORD. If none are set, sign.ps1 throws and the
      build aborts -- we don't want to silently ship unsigned bits.
    * -SkipSign: skips both signing steps. Useful for local dev iteration
      where you don't have (or don't want to use) a cert. CI does NOT pass
      this flag, so CI still proves the signing path works.

  For local development with a self-signed cert:
    .\sign\new-dev-cert.ps1
    $env:WEBP_TRAY_SIGN_THUMBPRINT = "<thumbprint printed by the script>"
    .\build-installer.ps1

  For local development without a cert:
    .\build-installer.ps1 -SkipSign

.OUTPUTS
  build\webp-tray-<version>.msi
#>
[CmdletBinding()]
param(
    [string]$Version = "0.1.0",
    [string]$Configuration = "release",
    [switch]$SkipSign
)

$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot

Write-Host "==> cargo build --$Configuration" -ForegroundColor Cyan
cargo build "--$Configuration"
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$ExePath = Join-Path $PSScriptRoot "target\$Configuration\webp-tray.exe"
if (-not (Test-Path $ExePath)) { throw "expected binary not found at $ExePath" }

if ($SkipSign) {
    Write-Host "==> skip sign exe (-SkipSign)" -ForegroundColor DarkYellow
} else {
    # Sign the exe BEFORE packaging into the MSI so the embedded copy is also
    # signed. (Signing the MSI signs the package container; the exe inside it
    # carries its own signature only if signed first.) sign.ps1 throws on any
    # failure -- we don't want to ever ship a half-signed build.
    Write-Host "==> sign exe" -ForegroundColor Cyan
    & "$PSScriptRoot\sign\sign.ps1" -Path $ExePath
    if ($LASTEXITCODE -ne 0) { throw "exe signing failed" }
}

Write-Host "==> dotnet tool restore" -ForegroundColor Cyan
dotnet tool restore | Out-Host
if ($LASTEXITCODE -ne 0) { throw "dotnet tool restore failed" }

$Out = Join-Path $PSScriptRoot "build\webp-tray-$Version.msi"
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null

Write-Host "==> wix build" -ForegroundColor Cyan
$IconPath = Join-Path $PSScriptRoot "assets\webp-tray.ico"
if (-not (Test-Path $IconPath)) {
    throw "missing $IconPath -- regenerate with: cargo run --example gen_icon"
}
dotnet wix build `
    "$PSScriptRoot\installer\installer.wxs" `
    -arch x64 `
    -d "Version=$Version" `
    -d "ExePath=$ExePath" `
    -d "IconPath=$IconPath" `
    -o $Out
if ($LASTEXITCODE -ne 0) { throw "wix build failed" }

if ($SkipSign) {
    Write-Host "==> skip sign msi (-SkipSign)" -ForegroundColor DarkYellow
} else {
    # Sign the MSI after WiX builds it. Without this the installer shows
    # "Unknown publisher" on the UAC / launch prompt.
    Write-Host "==> sign msi" -ForegroundColor Cyan
    & "$PSScriptRoot\sign\sign.ps1" -Path $Out
    if ($LASTEXITCODE -ne 0) { throw "msi signing failed" }
}

Write-Host ""
$label = if ($SkipSign) { "Built (UNSIGNED)" } else { "Built (signed)" }
Write-Host "${label}: $Out" -ForegroundColor Green
Write-Host "Install with: msiexec /i `"$Out`""
Write-Host "Silent install: msiexec /i `"$Out`" /qn"

