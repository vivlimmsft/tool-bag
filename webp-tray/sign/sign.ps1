#requires -Version 5.1
<#
.SYNOPSIS
  Sign a file (.exe or .msi) with a code-signing certificate, plus
  RFC 3161 timestamping so signatures stay valid past cert expiry.

.DESCRIPTION
  Signing is REQUIRED for release artefacts. Resolves the cert in this order:
    1. -Thumbprint param
    2. WEBP_TRAY_SIGN_THUMBPRINT env var (local cert store)
    3. WEBP_TRAY_SIGN_PFX + WEBP_TRAY_SIGN_PFX_PASSWORD env vars (CI path)
  If none are set, this script EXITS WITH AN ERROR. Run sign\new-dev-cert.ps1
  first (or set the env vars) before invoking build-installer.ps1.

.PARAMETER Path
  File to sign.

.PARAMETER Thumbprint
  Optional cert thumbprint to use (must be in CurrentUser\My or LocalMachine\My).

.PARAMETER TimestampUrl
  RFC 3161 timestamp authority. DigiCert's free URL is the default.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Path,
    [string]$Thumbprint,
    [string]$TimestampUrl = "http://timestamp.digicert.com"
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Path)) {
    throw "file not found: $Path"
}

# 1. Explicit -Thumbprint > 2. env thumbprint > 3. env PFX
if (-not $Thumbprint -and $env:WEBP_TRAY_SIGN_THUMBPRINT) {
    $Thumbprint = $env:WEBP_TRAY_SIGN_THUMBPRINT
}

$cert = $null
if ($Thumbprint) {
    $cert = Get-Item "Cert:\CurrentUser\My\$Thumbprint" -ErrorAction SilentlyContinue
    if (-not $cert) {
        $cert = Get-Item "Cert:\LocalMachine\My\$Thumbprint" -ErrorAction SilentlyContinue
    }
    if (-not $cert) {
        throw "thumbprint $Thumbprint not found in CurrentUser\My or LocalMachine\My"
    }
} elseif ($env:WEBP_TRAY_SIGN_PFX) {
    if (-not $env:WEBP_TRAY_SIGN_PFX_PASSWORD) {
        throw "WEBP_TRAY_SIGN_PFX is set but WEBP_TRAY_SIGN_PFX_PASSWORD is not"
    }
    $pfxPath = $env:WEBP_TRAY_SIGN_PFX
    if (-not (Test-Path -LiteralPath $pfxPath)) {
        throw "WEBP_TRAY_SIGN_PFX points at $pfxPath but the file doesn't exist"
    }
    $sec = ConvertTo-SecureString $env:WEBP_TRAY_SIGN_PFX_PASSWORD -AsPlainText -Force
    $cert = Get-PfxCertificate -FilePath $pfxPath -Password $sec
} else {
    throw @"
no signing certificate configured. Set ONE of:
  - `$env:WEBP_TRAY_SIGN_THUMBPRINT (cert in CurrentUser\My or LocalMachine\My)
  - `$env:WEBP_TRAY_SIGN_PFX + `$env:WEBP_TRAY_SIGN_PFX_PASSWORD (PFX file)
or pass -Thumbprint <hash> explicitly.
For local dev, run: sign\new-dev-cert.ps1
"@
}

Write-Host "[sign] signing $Path with $($cert.Subject) ($($cert.Thumbprint))" -ForegroundColor Cyan
$sig = Set-AuthenticodeSignature `
    -FilePath $Path `
    -Certificate $cert `
    -HashAlgorithm SHA256 `
    -TimestampServer $TimestampUrl

# Status semantics:
#   Valid        -> chain validates to a trusted root (real cert, or self-signed
#                   cert that's been moved to TrustedPublisher/Root).
#   UnknownError -> signature was applied but the chain doesn't terminate at a
#                   trusted root. Normal for a fresh self-signed cert. The file
#                   IS signed; users will see "Unknown publisher" until the cert
#                   is trusted on their machine.
#   anything else (HashMismatch, NotSigned, NotTrusted error w/o cert, ...)
#                -> signing genuinely failed.
$applied = ($null -ne $sig.SignerCertificate) -and `
           ($sig.SignerCertificate.Thumbprint -eq $cert.Thumbprint)
if (-not $applied) {
    throw "Set-AuthenticodeSignature did not attach our cert. status='$($sig.Status)' message='$($sig.StatusMessage)'"
}
if ($sig.Status -eq 'Valid') {
    Write-Host "[sign]   status: Valid (chain trusted)" -ForegroundColor Green
} elseif ($sig.Status -eq 'UnknownError') {
    Write-Host "[sign]   status: UnknownError — signature applied, but cert chain not trusted on this machine" -ForegroundColor Yellow
    Write-Host "[sign]   (this is normal for a self-signed cert; the file IS signed)"
} else {
    # Any other status with our cert attached: report but accept.
    Write-Host "[sign]   status: $($sig.Status) — signature applied" -ForegroundColor Yellow
    if ($sig.StatusMessage) { Write-Host "[sign]   message: $($sig.StatusMessage)" }
}
if ($sig.TimeStamperCertificate) {
    Write-Host "[sign]   timestamp: $($sig.TimeStamperCertificate.Subject)"
}
