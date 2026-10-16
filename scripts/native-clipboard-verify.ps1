<#
.SYNOPSIS
  Runs the automatable part of the Windows native clipboard round-trip checks.

.DESCRIPTION
  Verifies the representations that CI cannot prove on a real OS clipboard:
  CF_HTML capture fidelity, CF_HDROP file capture plus managed-copy
  materialization, and the copy-back path that rewrites the OS clipboard.

  The application directory is isolated by copying the executable into a fresh
  temp folder (resolve_project_directory() uses the executable's directory when
  writable). The script saves the current clipboard text to a file and restores
  it at the end; only CF_UNICODETEXT can be restored, other formats are lost.

  Not covered here, because it needs GUI interaction or a second device, is
  documented in docs/NATIVE_VERIFICATION.md: the WebView "format paste" HTML
  write, Explorer paste, and remote-media materialization.

.PARAMETER Exe
  Path to clipboard-desktop.exe. Defaults to the debug build.

.PARAMETER KeepData
  Leave the isolated run directory (database, logs, managed storage) in place.

.PARAMETER Force
  Run even when another clipboard-desktop instance is already running. That
  instance will also observe (and store) the test payloads.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/native-clipboard-verify.ps1
#>
[CmdletBinding()]
param(
  [string]$Exe = '',
  [switch]$KeepData,
  [switch]$Force
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (-not $Exe) {
  $scriptDirectory = Split-Path -Parent $MyInvocation.MyCommand.Path
  $Exe = Join-Path (Split-Path -Parent $scriptDirectory) 'src-tauri\target\debug\clipboard-desktop.exe'
}

if ($env:OS -ne 'Windows_NT') {
  throw 'This script exercises the Windows clipboard and only runs on Windows.'
}
if (-not (Test-Path -LiteralPath $Exe)) {
  throw "Executable not found: $Exe. Build it first (cargo build, or npm run tauri build)."
}
$Exe = (Resolve-Path -LiteralPath $Exe).Path
$pythonCommand = Get-Command python -ErrorAction SilentlyContinue
$python = if ($pythonCommand) { $pythonCommand.Source } else { $null }
if (-not $python) { throw 'python is required to read the isolated SQLite database.' }

$otherInstances = @(Get-Process -Name 'clipboard-desktop' -ErrorAction SilentlyContinue)
if ($otherInstances.Count -gt 0 -and -not $Force) {
  $ids = ($otherInstances | ForEach-Object { $_.Id }) -join ', '
  throw "Another clipboard-desktop instance is running (PID: $ids). Close it, or pass -Force to accept that it will also capture the test payloads."
}

Add-Type -Namespace ClipboardNative -Name Api -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true)] public static extern bool OpenClipboard(IntPtr hWndNewOwner);
[DllImport("user32.dll", SetLastError = true)] public static extern bool EmptyClipboard();
[DllImport("user32.dll", SetLastError = true)] public static extern bool CloseClipboard();
[DllImport("user32.dll", SetLastError = true)] public static extern IntPtr SetClipboardData(uint uFormat, IntPtr hMem);
[DllImport("user32.dll", SetLastError = true)] public static extern uint RegisterClipboardFormat(string lpszFormat);
[DllImport("user32.dll", SetLastError = true)] public static extern bool IsClipboardFormatAvailable(uint format);
[DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr GlobalAlloc(uint uFlags, UIntPtr dwBytes);
[DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr GlobalLock(IntPtr hMem);
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool GlobalUnlock(IntPtr hMem);
[DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr GlobalFree(IntPtr hMem);
'@ | Out-Null

$CF_UNICODETEXT = 13
$CF_HDROP = 15
$GMEM_MOVEABLE = 0x0002

function New-GlobalBuffer {
  param([byte[]]$Bytes)
  $handle = [ClipboardNative.Api]::GlobalAlloc($GMEM_MOVEABLE, [UIntPtr]::new([uint64]$Bytes.Length))
  if ($handle -eq [IntPtr]::Zero) { throw 'GlobalAlloc failed' }
  $pointer = [ClipboardNative.Api]::GlobalLock($handle)
  if ($pointer -eq [IntPtr]::Zero) { throw 'GlobalLock failed' }
  [System.Runtime.InteropServices.Marshal]::Copy($Bytes, 0, $pointer, $Bytes.Length)
  [void][ClipboardNative.Api]::GlobalUnlock($handle)
  return $handle
}

function ConvertTo-Utf16Bytes {
  param([string]$Text)
  $chars = [System.Text.Encoding]::Unicode.GetBytes($Text)
  $terminated = New-Object byte[] ($chars.Length + 2)
  [Array]::Copy($chars, $terminated, $chars.Length)
  return $terminated
}

function Get-DropFilesBytes {
  param([string[]]$Paths)
  $stream = New-Object System.IO.MemoryStream
  $writer = New-Object System.IO.BinaryWriter($stream)
  $writer.Write([uint32]20)   # pFiles
  $writer.Write([int32]0)     # pt.x
  $writer.Write([int32]0)     # pt.y
  $writer.Write([uint32]0)    # fNC
  $writer.Write([uint32]1)    # fWide
  foreach ($path in $Paths) {
    $writer.Write([System.Text.Encoding]::Unicode.GetBytes($path))
    $writer.Write([uint16]0)
  }
  $writer.Write([uint16]0)
  $writer.Flush()
  return $stream.ToArray()
}

function Get-CfHtmlBytes {
  param([string]$Fragment)
  $body = "<html><body><!--StartFragment-->$Fragment<!--EndFragment--></body></html>"
  $fragmentStart = $body.IndexOf('<!--StartFragment-->') + '<!--StartFragment-->'.Length
  $fragmentEnd = $body.IndexOf('<!--EndFragment-->')
  $headerTemplate = "Version:0.9`r`nStartHTML:0000000000`r`nEndHTML:0000000000`r`nStartFragment:0000000000`r`nEndFragment:0000000000`r`n"
  $headerLength = $headerTemplate.Length
  $header = $headerTemplate.
    Replace('StartHTML:0000000000', 'StartHTML:' + $headerLength.ToString('D10')).
    Replace('EndHTML:0000000000', 'EndHTML:' + ($headerLength + $body.Length).ToString('D10')).
    Replace('StartFragment:0000000000', 'StartFragment:' + ($headerLength + $fragmentStart).ToString('D10')).
    Replace('EndFragment:0000000000', 'EndFragment:' + ($headerLength + $fragmentEnd).ToString('D10'))
  return [System.Text.Encoding]::UTF8.GetBytes($header + $body)
}

function Set-ClipboardFormatsOnce {
  param([hashtable]$Formats)
  if (-not [ClipboardNative.Api]::OpenClipboard([IntPtr]::Zero)) {
    throw "OpenClipboard failed (win32 $([System.Runtime.InteropServices.Marshal]::GetLastWin32Error()))"
  }
  try {
    if (-not [ClipboardNative.Api]::EmptyClipboard()) {
      throw "EmptyClipboard failed (win32 $([System.Runtime.InteropServices.Marshal]::GetLastWin32Error()))"
    }
    foreach ($format in $Formats.Keys) {
      $handle = New-GlobalBuffer -Bytes $Formats[$format]
      if ([ClipboardNative.Api]::SetClipboardData([uint32]$format, $handle) -eq [IntPtr]::Zero) {
        $code = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
        [void][ClipboardNative.Api]::GlobalFree($handle)
        throw "SetClipboardData failed for format $format (win32 $code)"
      }
    }
  } finally {
    [void][ClipboardNative.Api]::CloseClipboard()
  }
}

# Another process (including a concurrently running dev build) can hold the
# clipboard open for a moment; retry like the production write path does.
function Set-ClipboardFormats {
  param([hashtable]$Formats)
  $lastError = $null
  for ($attempt = 0; $attempt -lt 40; $attempt++) {
    try {
      Set-ClipboardFormatsOnce -Formats $Formats
      return
    } catch {
      $lastError = $_
      Start-Sleep -Milliseconds 100
    }
  }
  throw $lastError
}

function Invoke-DbQuery {
  param([string]$Database, [string]$Query)
  $json = & $python $script:queryScript $Database $Query
  if ($LASTEXITCODE -ne 0) { throw "database query failed: $Query" }
  return ($json | ConvertFrom-Json)
}

function Invoke-IsolatedCli {
  param([string]$CliExe, [string]$RunDirectory, [string[]]$Arguments)
  $stdout = Join-Path $RunDirectory 'cli.out.log'
  $stderr = Join-Path $RunDirectory 'cli.err.log'
  $proc = Start-Process -FilePath $CliExe -ArgumentList $Arguments -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -Wait -NoNewWindow
  return [pscustomobject]@{
    ExitCode = $proc.ExitCode
    Stdout   = (Get-Content -LiteralPath $stdout -Raw -ErrorAction SilentlyContinue)
    Stderr   = (Get-Content -LiteralPath $stderr -Raw -ErrorAction SilentlyContinue)
  }
}

$runDirectory = Join-Path $env:TEMP ("clipboard-native-" + [guid]::NewGuid().ToString('N'))
$backupFile = Join-Path $env:TEMP ("clipboard-backup-" + [guid]::NewGuid().ToString('N') + '.txt')
$results = New-Object System.Collections.Generic.List[object]
$guiProcess = $null

function Add-Result {
  param([string]$Check, [bool]$Passed, [string]$Detail)
  $results.Add([pscustomobject]@{ Check = $Check; Passed = $Passed; Detail = $Detail })
  $label = if ($Passed) { 'PASS' } else { 'FAIL' }
  Write-Host ("  [{0}] {1} - {2}" -f $label, $Check, $Detail)
}

try {
  New-Item -ItemType Directory -Path $runDirectory | Out-Null
  $clipboardText = ''
  try { $clipboardText = (Get-Clipboard -Raw -ErrorAction Stop) } catch { $clipboardText = '' }
  [System.IO.File]::WriteAllText($backupFile, $clipboardText)

  $script:queryScript = Join-Path $runDirectory 'query.py'
  [System.IO.File]::WriteAllText($script:queryScript, @'
import sqlite3, sys, json
connection = sqlite3.connect(sys.argv[1])
connection.row_factory = sqlite3.Row
print(json.dumps([dict(row) for row in connection.execute(sys.argv[2])]))
'@)

  Write-Host "Isolated run directory: $runDirectory"
  $isolatedExe = Join-Path $runDirectory 'clipboard-desktop.exe'
  Copy-Item -LiteralPath $Exe -Destination $isolatedExe
  $database = Join-Path $runDirectory 'storage\database\clipboard.sqlite3'

  $guiProcess = Start-Process -FilePath $isolatedExe `
    -RedirectStandardOutput (Join-Path $runDirectory 'gui.out.log') `
    -RedirectStandardError (Join-Path $runDirectory 'gui.err.log') `
    -PassThru
  Write-Host "Launched isolated GUI (PID $($guiProcess.Id)); waiting for the database..."
  $deadline = (Get-Date).AddSeconds(60)
  while ((Get-Date) -lt $deadline -and -not (Test-Path -LiteralPath $database)) {
    Start-Sleep -Milliseconds 250
  }
  if (-not (Test-Path -LiteralPath $database)) { throw "the isolated application never created $database" }
  Start-Sleep -Seconds 2

  # Check A: CF_HTML capture fidelity.
  $marker = 'marker-' + [guid]::NewGuid().ToString('N')
  $fragment = "<b>$marker</b>"
  $htmlFormat = [ClipboardNative.Api]::RegisterClipboardFormat('HTML Format')
  if ($htmlFormat -eq 0) { throw 'RegisterClipboardFormat(HTML Format) failed' }
  Set-ClipboardFormats @{
    $htmlFormat     = (Get-CfHtmlBytes -Fragment $fragment)
    $CF_UNICODETEXT = (ConvertTo-Utf16Bytes -Text $marker)
  }
  Start-Sleep -Seconds 3
  $htmlRows = Invoke-DbQuery -Database $database -Query "select id, html_content from clipboard_items where html_content is not null order by created_at_ms desc"
  $htmlMatch = @($htmlRows | Where-Object { $_.html_content -like "*$marker*" })
  Add-Result -Check 'html-capture' -Passed ($htmlMatch.Count -ge 1) `
    -Detail ($(if ($htmlMatch.Count -ge 1) { "stored html_content for $marker" } else { "no html_content row contained $marker" }))

  # Check B: CF_HDROP file capture plus managed-copy materialization.
  $sourceFile = Join-Path $runDirectory ("marker-$([guid]::NewGuid().ToString('N')).txt")
  $sourceText = 'file-marker-' + [guid]::NewGuid().ToString('N')
  [System.IO.File]::WriteAllText($sourceFile, $sourceText)
  $fileMarker = Split-Path -Leaf $sourceFile
  Set-ClipboardFormats @{
    $CF_HDROP       = (Get-DropFilesBytes -Paths @($sourceFile))
    $CF_UNICODETEXT = (ConvertTo-Utf16Bytes -Text $sourceFile)
  }
  Start-Sleep -Seconds 3
  $fileRows = Invoke-DbQuery -Database $database -Query "select id, kind, resource_path, metadata_json from clipboard_items where kind = 'file' order by created_at_ms desc"
  $fileRow = $fileRows | Where-Object { $_.metadata_json -like "*$fileMarker*" } | Select-Object -First 1
  $managedExists = $false
  if ($fileRow -and $fileRow.resource_path) {
    $managedExists = Test-Path -LiteralPath $fileRow.resource_path
  }
  Add-Result -Check 'file-capture' -Passed ([bool]$fileRow) `
    -Detail ($(if ($fileRow) { "stored file record $($fileRow.id)" } else { "no file record for $fileMarker" }))
  Add-Result -Check 'file-materialize' -Passed $managedExists `
    -Detail ($(if ($managedExists) { "managed copy exists at $($fileRow.resource_path)" } else { 'managed copy missing for record' }))

  # Check C: copy-back rewrites the OS clipboard as text.
  if ($htmlMatch.Count -ge 1) {
    $copyId = $htmlMatch[0].id
    $copy = Invoke-IsolatedCli -CliExe $isolatedExe -RunDirectory $runDirectory -Arguments @('copy', '--query', $copyId)
    Start-Sleep -Milliseconds 600
    $restored = (Get-Clipboard -Raw)
    $clipboardHasMarker = $restored -like "*$marker*"
    Add-Result -Check 'copy-back-text' -Passed ($copy.ExitCode -eq 0 -and $clipboardHasMarker) `
      -Detail ("cli exit={0}, clipboard contains marker={1}" -f $copy.ExitCode, $clipboardHasMarker)
  }

  # Check D: the file copy-back writes CF_HDROP.
  if ($fileRow) {
    $copy = Invoke-IsolatedCli -CliExe $isolatedExe -RunDirectory $runDirectory -Arguments @('copy', '--query', $fileRow.id)
    Start-Sleep -Milliseconds 600
    $hasDrop = [ClipboardNative.Api]::IsClipboardFormatAvailable([uint32]$CF_HDROP)
    Add-Result -Check 'copy-back-cf-hdrop' -Passed ($copy.ExitCode -eq 0 -and $hasDrop) `
      -Detail ("cli exit={0}, CF_HDROP available={1}" -f $copy.ExitCode, $hasDrop)
  }
} finally {
  if ($guiProcess -and -not $guiProcess.HasExited) {
    Stop-Process -Id $guiProcess.Id -Force -ErrorAction SilentlyContinue
  }
  Start-Sleep -Milliseconds 500
  try {
    if (Test-Path -LiteralPath $backupFile) {
      Set-Clipboard -Value ([System.IO.File]::ReadAllText($backupFile))
    }
  } catch {
    Write-Warning "failed to restore the clipboard: $_"
  }
  Remove-Item -LiteralPath $backupFile -Force -ErrorAction SilentlyContinue
  if (-not $KeepData) {
    Remove-Item -LiteralPath $runDirectory -Recurse -Force -ErrorAction SilentlyContinue
  }
}

$failed = @($results | Where-Object { -not $_.Passed })
Write-Host ''
if ($failed.Count -eq 0) {
  Write-Host ("All {0} checks passed." -f $results.Count)
  exit 0
}
Write-Host ("{0} of {1} checks failed." -f $failed.Count, $results.Count)
exit 1
