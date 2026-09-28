<#
.SYNOPSIS
  Provisions a disposable rustfs S3 server and runs the opt-in real-S3 sync
  tests against it.

.DESCRIPTION
  The sync engine's transport contract (SigV4 signing, path-style addressing,
  conditional writes, ETag quoting, ListObjectsV2 continuation tokens) can only
  be proven against a real S3-compatible server. `MemoryStore` covers engine
  semantics but not the wire.

  This script downloads a pinned rustfs release, verifies its published
  SHA-256, starts it on a loopback port, exports the CLIPBOARD_S3_TEST_* values
  the tests read, runs the requested tests, and stops the server.

  Nothing is installed system-wide and no binary is committed to the
  repository: the archive lands under a gitignored tools directory.

.PARAMETER Test
  Which test target to run.
    smoke   - the fast real-S3 correctness smoke (default, ~10s)
    bench   - the 1001-segment pagination benchmark (~60s, unoptimized local
              profile is much slower than the documented release baseline)
    all     - both

.PARAMETER Port
  Loopback port for rustfs. Default 9200.

.PARAMETER KeepServer
  Leave rustfs running after the tests finish (useful for manual poking).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/sync-s3-test.ps1

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/sync-s3-test.ps1 -Test bench
#>
[CmdletBinding()]
param(
  [ValidateSet('smoke', 'bench', 'all')]
  [string]$Test = 'smoke',
  [int]$Port = 9200,
  [switch]$KeepServer
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# Pinned release. The checksum is asserted against the upstream SHA256SUMS file
# for this exact tag, so a compromised or substituted asset fails the run.
$RustfsVersion = '1.0.1-preview.11'
$RustfsAsset = "rustfs-windows-x86_64-v$RustfsVersion.zip"
$DownloadBase = "https://github.com/rustfs/rustfs/releases/download/$RustfsVersion"

$Bucket = 'clipboard-sync-rustfs-test'
$AccessKey = 'clipboardsmoke'
$SecretKey = 'clipboard-smoke-secret-0123456789'
$Password = 'clipboard-smoke-sync-password'

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$ToolsDir = Join-Path $ProjectRoot '.tools\rustfs'
$Server = Join-Path $ToolsDir 'rustfs.exe'
$DataDir = Join-Path $ToolsDir 'data'

function Get-PinnedAsset {
  param(
    [Parameter(Mandatory)] [string]$Url,
    [Parameter(Mandatory)] [string]$Destination
  )
  if (Test-Path -LiteralPath $Destination) {
    Write-Host "  cached: $(Split-Path -Leaf $Destination)"
    return
  }
  Write-Host "  downloading $(Split-Path -Leaf $Url)"
  $partial = "$Destination.partial"
  Invoke-WebRequest -Uri $Url -OutFile $partial -UseBasicParsing -TimeoutSec 900
  Move-Item -LiteralPath $partial -Destination $Destination -Force
}

function Install-Rustfs {
  if (Test-Path -LiteralPath $Server) {
    Write-Host "rustfs already provisioned at $Server"
    return
  }
  Write-Host "provisioning rustfs $RustfsVersion"
  New-Item -ItemType Directory -Force -Path $ToolsDir | Out-Null
  $archive = Join-Path $ToolsDir $RustfsAsset
  $sums = Join-Path $ToolsDir 'SHA256SUMS'

  Get-PinnedAsset -Url "$DownloadBase/SHA256SUMS" -Destination $sums
  Get-PinnedAsset -Url "$DownloadBase/$RustfsAsset" -Destination $archive

  $expected = $null
  foreach ($line in (Get-Content -LiteralPath $sums)) {
    if ($line -match '^([0-9a-fA-F]{64})\s+\*?' + [regex]::Escape($RustfsAsset) + '$') {
      $expected = $Matches[1].ToLowerInvariant()
      break
    }
  }
  if (-not $expected) {
    throw "upstream SHA256SUMS has no entry for $RustfsAsset"
  }
  $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
  if ($actual -ne $expected) {
    Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
    throw "rustfs archive checksum mismatch: expected $expected, got $actual"
  }
  Write-Host "  checksum verified ($expected)"

  $extract = Join-Path $ToolsDir 'extract'
  if (Test-Path -LiteralPath $extract) { Remove-Item -LiteralPath $extract -Recurse -Force }
  Expand-Archive -LiteralPath $archive -DestinationPath $extract -Force
  $binary = Get-ChildItem -LiteralPath $extract -Filter 'rustfs.exe' -Recurse | Select-Object -First 1
  if (-not $binary) { throw 'rustfs.exe not found in the release archive' }
  Move-Item -LiteralPath $binary.FullName -Destination $Server -Force
  Remove-Item -LiteralPath $extract -Recurse -Force
  Remove-Item -LiteralPath $archive -Force
}

function Start-Rustfs {
  param([int]$BindPort)
  $existing = Get-NetTCPConnection -LocalPort $BindPort -State Listen -ErrorAction SilentlyContinue
  if ($existing) {
    Write-Host "reusing the server already listening on 127.0.0.1:$BindPort"
    return $null
  }
  New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
  $out = Join-Path $ToolsDir 'rustfs.out.log'
  $err = Join-Path $ToolsDir 'rustfs.err.log'
  $process = Start-Process -FilePath $Server `
    -ArgumentList @(
      'server',
      '--address', "127.0.0.1:$BindPort",
      '--access-key', $AccessKey,
      '--secret-key', $SecretKey,
      '--region', 'us-east-1',
      $DataDir
    ) `
    -RedirectStandardOutput $out -RedirectStandardError $err -PassThru -WindowStyle Hidden
  Write-Host "  started rustfs (pid $($process.Id)) on 127.0.0.1:$BindPort"
  for ($attempt = 0; $attempt -lt 40; $attempt++) {
    try {
      $response = Invoke-WebRequest -Uri "http://127.0.0.1:$BindPort/minio/health/live" -UseBasicParsing -TimeoutSec 5
      if ($response.StatusCode -eq 200) { return $process }
    } catch { }
    Start-Sleep -Milliseconds 250
  }
  if (Test-Path -LiteralPath $err) { Get-Content -LiteralPath $err -Tail 20 | Write-Host }
  throw "rustfs did not become healthy on 127.0.0.1:$BindPort"
}

function Invoke-SyncTests {
  param([string]$Filter, [string]$Label)
  Write-Host ""
  Write-Host "== $Label =="
  $arguments = @(
    'test', '--manifest-path', (Join-Path $ProjectRoot 'src-tauri\Cargo.toml'),
    '--lib', '--', '--ignored', '--nocapture', '--test-threads=1'
  )
  if ($Filter) { $arguments += $Filter }
  & cargo @arguments
  if ($LASTEXITCODE -ne 0) { throw "$Label failed (cargo exit $LASTEXITCODE)" }
}

Install-Rustfs
$process = Start-Rustfs -BindPort $Port

$env:CLIPBOARD_S3_TEST_ENDPOINT = "http://127.0.0.1:$Port"
$env:CLIPBOARD_S3_TEST_REGION = 'us-east-1'
$env:CLIPBOARD_S3_TEST_BUCKET = $Bucket
$env:CLIPBOARD_S3_TEST_ACCESS_KEY = $AccessKey
$env:CLIPBOARD_S3_TEST_SECRET_KEY = $SecretKey
$env:CLIPBOARD_S3_TEST_PASSWORD = $Password

try {
  if ($Test -in @('smoke', 'all')) {
    Invoke-SyncTests -Filter 'sync::v1::s3_smoke' -Label 'real-S3 smoke'
  }
  if ($Test -in @('bench', 'all')) {
    Invoke-SyncTests -Filter 'sync::v1::scale_bench::s3_segment_listing_paginates' -Label 'real-S3 pagination benchmark'
  }
  if ($Test -eq 'all') {
    Invoke-SyncTests -Filter 'clipboard_sync::s3::tests::disposable_s3' -Label 'real-S3 transport round trip'
  }
  Write-Host ""
  Write-Host 'real-S3 tests passed'
} finally {
  if ($process -and -not $KeepServer) {
    Write-Host "stopping rustfs (pid $($process.Id))"
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
  } elseif ($process) {
    Write-Host "rustfs left running on 127.0.0.1:$Port (pid $($process.Id))"
  }
}
