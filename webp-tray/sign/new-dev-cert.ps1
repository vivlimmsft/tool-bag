#requires -Version 5.1
<#
.SYNOPSIS
  Create a self-signed code-signing certificate for webp-tray, install it
  into the current user's Personal store, and (optionally) export it as a
  password-protected PFX for use by CI.

.DESCRIPTION
  Self-signing does NOT bypass SmartScreen. It does:
    * stamp the binary with a stable identity ("webp-tray dev cert")
    * let users explicitly trust the publisher to silence repeated prompts
    * provide a drop-in path to swap for a real cert later (same script,
      different thumbprint)

  The cert is created with the standard Authenticode key-usage flags and
  a 5-year validity window.

.PARAMETER Subject
  Cert subject. Default: "CN=webp-tray dev cert, O=vivlim".

.PARAMETER PfxPath
  Path to write the exported PFX. Default: .\sign\dev-cert.pfx (gitignored).

.PARAMETER Password
  Password for the PFX. Required when -ExportPfx is set. CI consumes this
  via the WEBP_TRAY_SIGN_PFX_PASSWORD secret.

.PARAMETER ExportPfx
  Also export the new cert + private key as a PFX file (so you can ship it
  to CI). Off by default — local signing reads the cert directly from your
  cert store via thumbprint.
#>
[CmdletBinding()]
param(
    [string]$Subject = "CN=webp-tray dev cert, O=vivlim",
    [string]$PfxPath = (Join-Path $PSScriptRoot "dev-cert.pfx"),
    [securestring]$Password,
    [switch]$ExportPfx
)

$ErrorActionPreference = 'Stop'

Write-Host "==> Creating self-signed code-signing cert" -ForegroundColor Cyan
$cert = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $Subject `
    -KeyUsage DigitalSignature `
    -KeyAlgorithm RSA `
    -KeyLength 2048 `
    -HashAlgorithm SHA256 `
    -CertStoreLocation Cert:\CurrentUser\My `
    -NotAfter (Get-Date).AddYears(5)

Write-Host "  thumbprint: $($cert.Thumbprint)" -ForegroundColor Green
Write-Host "  subject:    $($cert.Subject)"

Write-Host ""
Write-Host "Tip: to silence 'unknown publisher' prompts on this machine, run:" -ForegroundColor Yellow
Write-Host "  Move-Item Cert:\CurrentUser\My\$($cert.Thumbprint) Cert:\CurrentUser\TrustedPublisher" -ForegroundColor Yellow
Write-Host ""

if ($ExportPfx) {
    if (-not $Password) {
        $Password = Read-Host -AsSecureString -Prompt "PFX export password"
    }
    Export-PfxCertificate `
        -Cert "Cert:\CurrentUser\My\$($cert.Thumbprint)" `
        -FilePath $PfxPath `
        -Password $Password | Out-Null
    Write-Host "==> Wrote PFX: $PfxPath" -ForegroundColor Green
    Write-Host ""
    Write-Host "For GitHub Actions, set these repository secrets:" -ForegroundColor Yellow
    $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($PfxPath))
    Write-Host "  WEBP_TRAY_CERT_PFX_BASE64 = (the base64 below, all on one line)"
    Write-Host "  WEBP_TRAY_CERT_PASSWORD   = (the password you just entered)"
    Write-Host ""
    Write-Host "----- BEGIN BASE64 -----"
    Write-Host $b64
    Write-Host "----- END BASE64 -----"
}

Write-Host ""
Write-Host "Set this env var so build-installer.ps1 picks up the cert:" -ForegroundColor Cyan
Write-Host "  `$env:WEBP_TRAY_SIGN_THUMBPRINT = '$($cert.Thumbprint)'"
