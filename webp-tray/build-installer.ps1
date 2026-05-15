#requires -Version 5.1
<#
.SYNOPSIS
  Build the webp-tray release binary and a per-user MSI installer, signing
  both with the configured Authenticode certificate.

.DESCRIPTION
  1. cargo build --release
  2. sign the .exe (REQUIRED — fails if no cert configured)
  3. dotnet tool restore   (pulls WiX 6 into this project)
  4. wix build installer/installer.wxs
  5. sign the .msi (REQUIRED)

  Cert resolution: see sign\sign.ps1 (-Thumbprint, WEBP_TRAY_SIGN_THUMBPRINT,
  or WEBP_TRAY_SIGN_PFX + WEBP_TRAY_SIGN_PFX_PASSWORD).

  For local development without a cert yet:
    .\sign\new-dev-cert.ps1
    $env:WEBP_TRAY_SIGN_THUMBPRINT = "<thumbprint printed by the script>"
    .\build-installer.ps1

.OUTPUTS
  build\webp-tray-<version>.msi (signed)
#>
[CmdletBinding()]
param(
    [string]$Version = "0.1.0",
    [string]$Configuration = "release"
)

$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot

Write-Host "==> cargo build --$Configuration" -ForegroundColor Cyan
cargo build "--$Configuration"
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$ExePath = Join-Path $PSScriptRoot "target\$Configuration\webp-tray.exe"
if (-not (Test-Path $ExePath)) { throw "expected binary not found at $ExePath" }

# Sign the exe BEFORE packaging into the MSI so the embedded copy is also
# signed. (Signing the MSI signs the package container; the exe inside it
# carries its own signature only if signed first.) sign.ps1 throws on any
# failure — we don't want to ever ship an unsigned binary.
Write-Host "==> sign exe" -ForegroundColor Cyan
& "$PSScriptRoot\sign\sign.ps1" -Path $ExePath
if ($LASTEXITCODE -ne 0) { throw "exe signing failed" }

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

# Sign the MSI after WiX builds it. Without this the installer shows
# "Unknown publisher" on the UAC / launch prompt.
Write-Host "==> sign msi" -ForegroundColor Cyan
& "$PSScriptRoot\sign\sign.ps1" -Path $Out
if ($LASTEXITCODE -ne 0) { throw "msi signing failed" }

Write-Host ""
Write-Host "Built (signed): $Out" -ForegroundColor Green
Write-Host "Install with: msiexec /i `"$Out`""
Write-Host "Silent install: msiexec /i `"$Out`" /qn"
