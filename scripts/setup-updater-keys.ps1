# One-time Berryworks updater signing setup.
# Generates a keypair outside the repo, writes the public key into tauri.conf.json,
# and optionally uploads the private key to GitHub Actions secrets.
#
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts/setup-updater-keys.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/setup-updater-keys.ps1 -SetGithubSecrets

param(
  [switch]$SetGithubSecrets,
  [string]$KeyPath = "$env:USERPROFILE\.tauri\berryworks.key",
  [string]$Password = "berryworks-ci"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

New-Item -ItemType Directory -Force -Path (Split-Path $KeyPath) | Out-Null

$env:CI = "true"
Write-Host "Generating keypair at $KeyPath ..."
& npm run tauri -- signer generate -w $KeyPath -f --ci --password $Password
if ($LASTEXITCODE -ne 0) { throw "tauri signer generate failed ($LASTEXITCODE)" }

$pubPath = "$KeyPath.pub"
if (-not (Test-Path $pubPath)) { throw "Missing public key: $pubPath" }
$pubkey = (Get-Content $pubPath -Raw).Trim()
if (-not $pubkey) { throw "Public key file was empty" }

$confPath = Join-Path $Root "src-tauri\tauri.conf.json"
$conf = Get-Content $confPath -Raw
if ($conf -notmatch '"pubkey"\s*:') {
  throw "tauri.conf.json is missing plugins.updater.pubkey"
}
$conf = [regex]::Replace(
  $conf,
  '("pubkey"\s*:\s*")[^"]*(")',
  "`${1}$pubkey`${2}"
)
Set-Content -Path $confPath -Value $conf -NoNewline
Write-Host "Wrote pubkey into src-tauri/tauri.conf.json"

if ($SetGithubSecrets) {
  if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw "gh CLI not found - install GitHub CLI or set secrets manually"
  }
  Get-Content $KeyPath -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY
  $Password | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  Write-Host "Set GitHub secrets TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
} else {
  Write-Host ""
  Write-Host "Next: upload the private key to GitHub (do not commit it):"
  Write-Host "  Get-Content $KeyPath -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY"
  Write-Host "  '$Password' | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
  Write-Host "Or re-run this script with -SetGithubSecrets"
}

Write-Host "Done. Keep $KeyPath private and backed up - losing it breaks in-app updates for existing installs."
