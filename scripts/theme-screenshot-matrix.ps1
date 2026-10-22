<#
.SYNOPSIS
  Captures the native main window across themes, widths and window effects.

.DESCRIPTION
  For each scenario it launches an isolated copy of the application with a
  seeded conf.json (theme, window effect, transparency), seeds a few rows
  straight into the isolated SQLite database, and grabs the real window with
  scripts/capture-window.ps1. The result is the evidence TODO.md section B asks
  for: dark/light/custom at normal and 520px widths, plus the Windows text
  transparency toggle combined with acrylic/mica.

  The main window enforces a 730px minimum (tauri.conf.json, the backend's
  MAIN_WINDOW_MIN_WIDTH and geometry.rs). The "narrow" scenarios therefore use
  that enforced minimum, and one extra scenario forces 520px purely to record
  the below-minimum stress behaviour. The 520px responsive breakpoint lives in
  the detail panel CSS and the browser fixtures, not in a resizable native main
  window; the real narrow native surface is the 320px float panel.

  Seeding goes through the database, not the clipboard, so this script does not
  disturb the OS clipboard.

.PARAMETER Exe
  Path to clipboard-desktop.exe. Defaults to the debug build.

.PARAMETER OutputDirectory
  Where the PNGs are written. Defaults to a temp folder.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/theme-screenshot-matrix.ps1
#>
[CmdletBinding()]
param(
  [string]$Exe = '',
  [string]$OutputDirectory = (Join-Path $env:TEMP 'clipboard-theme-shots')
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if ($env:OS -ne 'Windows_NT') { throw 'This script captures Windows windows and only runs on Windows.' }
if (-not $Exe) {
  $scriptDirectory = Split-Path -Parent $MyInvocation.MyCommand.Path
  $Exe = Join-Path (Split-Path -Parent $scriptDirectory) 'src-tauri\target\debug\clipboard-desktop.exe'
}
if (-not (Test-Path -LiteralPath $Exe)) { throw "Executable not found: $Exe" }
$Exe = (Resolve-Path -LiteralPath $Exe).Path

$pythonCommand = Get-Command python -ErrorAction SilentlyContinue
$python = if ($pythonCommand) { $pythonCommand.Source } else { $null }
if (-not $python) { throw 'python is required to seed the isolated SQLite database.' }

$captureScript = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'capture-window.ps1'
if (-not (Test-Path -LiteralPath $captureScript)) { throw "missing $captureScript" }

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

# The applied palette comes from `themeColors`, not the `theme` selector: the
# Theme settings panel copies a preset into `themeColors` when the user picks
# dark/light, so seeding only `theme` renders the default dark palette.
$darkColors = @{
  bg = '#1b1b1b'; settingsBg = '#1b1b1b'; accent = '#2576f8'; textPrimary = '#f5f5f5'
  textMuted = '#999999'; border = '#3a3a3a'; cardBg = '#1e1e1e'; surfaceBg = '#1e1e1e'
  statusBarBg = '#181818'; hoverBg = '#2c2c2c'; inputBg = '#1a1a1a'; textSecondary = '#b2b2b2'
  textFaint = '#6e6e6e'; placeholderColor = '#6e6e6e'; borderSubtle = '#292929'
  selectionColor = '#4c8dff'; successColor = '#51b96b'; dangerColor = '#e85d5d'
  warningColor = '#e2c05d'; scrollbarColor = '#858585'
}
$lightColors = @{
  bg = '#f5f5f5'; settingsBg = '#f7f8fa'; accent = '#1a6ae8'; textPrimary = '#1a1a1a'
  textMuted = '#666666'; border = '#cccccc'; cardBg = '#ffffff'; surfaceBg = '#ffffff'
  statusBarBg = '#e8e8e8'; hoverBg = '#e0e0e0'; inputBg = '#f0f0f0'; textSecondary = '#444444'
  textFaint = '#999999'; placeholderColor = '#aaaaaa'; borderSubtle = '#dddddd'
  selectionColor = '#1a73e8'; successColor = '#388e3c'; dangerColor = '#d32f2f'
  warningColor = '#f9a825'; scrollbarColor = '#aaaaaa'
}
$customColors = @{
  bg = '#0b1f2a'; settingsBg = '#0b1f2a'; accent = '#18c07a'; textPrimary = '#e6fff5'
  textMuted = '#8fb3ad'; border = '#1f5566'; cardBg = '#12303d'; surfaceBg = '#12303d'
  statusBarBg = '#092028'; hoverBg = '#1b4250'; inputBg = '#0e2630'; textSecondary = '#bfe3d8'
  textFaint = '#6f948d'; placeholderColor = '#6f948d'; borderSubtle = '#17414e'
  selectionColor = '#18c07a'; successColor = '#18c07a'; dangerColor = '#e06a5a'
  warningColor = '#d8c06a'; scrollbarColor = '#4f7d7a'
}

$scenarios = @(
  @{ Name = 'dark-normal';  Theme = 'dark';   Width = 746; Effect = 'off';     Opacity = 95; AffectsText = $false },
  @{ Name = 'dark-min730';  Theme = 'dark';   Width = 730; Effect = 'off';     Opacity = 95; AffectsText = $false },
  @{ Name = 'light-normal'; Theme = 'light';  Width = 746; Effect = 'off';     Opacity = 95; AffectsText = $false },
  @{ Name = 'light-min730'; Theme = 'light';  Width = 730; Effect = 'off';     Opacity = 95; AffectsText = $false },
  @{ Name = 'custom-normal'; Theme = 'custom'; Width = 746; Effect = 'off';    Opacity = 95; AffectsText = $false },
  @{ Name = 'custom-min730'; Theme = 'custom'; Width = 730; Effect = 'off';    Opacity = 95; AffectsText = $false },
  @{ Name = 'dark-acrylic-textalpha'; Theme = 'dark';  Width = 746; Effect = 'acrylic'; Opacity = 85; AffectsText = $true },
  @{ Name = 'light-mica-textalpha';   Theme = 'light'; Width = 746; Effect = 'mica';    Opacity = 85; AffectsText = $true },
  @{ Name = 'dark-below-min-520'; Theme = 'dark'; Width = 520; Effect = 'off'; Opacity = 95; AffectsText = $false }
)

$seedScript = @'
import sqlite3, sys, time, json
connection = sqlite3.connect(sys.argv[1])
now = int(time.time() * 1000)
rows = [
    ("seed-1", "text", "Release checklist", "Release checklist\n- verify the native clipboard\n- capture the theme matrix", "https://example.com/release", "text-hash-1", "Explorer", 92, 0),
    ("seed-2", "link", "example.com/docs", "https://example.com/docs", None, "link-hash-2", "Edge", 26, 0),
    ("seed-3", "text", "API token rotation notes", "API token rotation notes\nrotate the namespace, then re-sync", None, "text-hash-3", "Code", 64, 1),
    ("seed-4", "text", "Unicode and quoting sample", "Unicode sample: 你好，世界 — “quoted” and emoji-free", None, "text-hash-4", "Notepad", 58, 0),
    ("seed-5", "text", "Long single line", "single line " * 40, None, "text-hash-5", "Terminal", 480, 0),
]
for index, (item_id, kind, title, text, html, content_hash, source, size, favorite) in enumerate(rows):
    created = now - index * 60000
    connection.execute(
        "INSERT OR REPLACE INTO clipboard_items (id, kind, title, text_content, html_content, rtf_content, content_hash, source_app, size_bytes, created_at_ms, last_used_at_ms, is_favorite, deleted, metadata_json) "
        "VALUES (?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, 0, '{}')",
        (item_id, kind, title, text, html, content_hash, source, size, created, created, favorite),
    )
connection.commit()
connection.close()
'@

function New-ScenarioConfiguration {
  param([hashtable]$Scenario)
  $general = [ordered]@{
    language                 = 'zh-CN'
    theme                    = $Scenario.Theme
    windowEffect             = $Scenario.Effect
    windowTransparency       = $Scenario.Opacity
    windowOpacityAffectsText = $Scenario.AffectsText
    useSystemTitleBar        = $true
    pinCopiedToTop           = $true
    onboardingCompleted      = $true
  }
  $general.themeColors = switch ($Scenario.Theme) {
    'light' { $lightColors }
    'custom' { $customColors }
    default { $darkColors }
  }
  # Pause capture so the isolated instance does not pull whatever happens to be
  # on the real clipboard into the seeded history during the run.
  return ([ordered]@{ general = $general; privacy = @{ paused = $true } } | ConvertTo-Json -Depth 6)
}

$captured = New-Object System.Collections.Generic.List[string]

foreach ($scenario in $scenarios) {
  $runDirectory = Join-Path $env:TEMP ("clipboard-shot-" + [guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $runDirectory | Out-Null
  New-Item -ItemType Directory -Path (Join-Path $runDirectory 'conf') -Force | Out-Null
  Copy-Item -LiteralPath $Exe -Destination (Join-Path $runDirectory 'clipboard-desktop.exe')
  # ConfigStore rejects a BOM-prefixed file, so write without one.
  [System.IO.File]::WriteAllText(
    (Join-Path $runDirectory 'conf\conf.json'),
    (New-ScenarioConfiguration -Scenario $scenario)
  )
  $seedFile = Join-Path $runDirectory 'seed.py'
  [System.IO.File]::WriteAllText($seedFile, $seedScript)

  $gui = Start-Process -FilePath (Join-Path $runDirectory 'clipboard-desktop.exe') `
    -RedirectStandardOutput (Join-Path $runDirectory 'gui.out.log') `
    -RedirectStandardError (Join-Path $runDirectory 'gui.err.log') `
    -PassThru
  try {
    $database = Join-Path $runDirectory 'storage\database\clipboard.sqlite3'
    $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline -and -not (Test-Path -LiteralPath $database)) {
      Start-Sleep -Milliseconds 250
    }
    if (-not (Test-Path -LiteralPath $database)) { throw "scenario $($scenario.Name): database was never created" }
    & $python $seedFile $database
    if ($LASTEXITCODE -ne 0) { throw "scenario $($scenario.Name): seeding failed" }

    # Give the external-change worker time to notice the seed and repaint.
    Start-Sleep -Seconds 3
    $outputPath = Join-Path $OutputDirectory ("$($scenario.Name).png")
    & powershell -NoProfile -ExecutionPolicy Bypass -File $captureScript `
      -ProcessId $gui.Id -OutputPath $outputPath -Width $scenario.Width | Write-Host
    if (-not (Test-Path -LiteralPath $outputPath)) { throw "scenario $($scenario.Name): capture produced no file" }
    $captured.Add($outputPath)
    Write-Host ("Captured {0}" -f $scenario.Name)
  } finally {
    if (-not $gui.HasExited) { Stop-Process -Id $gui.Id -Force -ErrorAction SilentlyContinue }
  }
}

Write-Host ''
Write-Host ("{0} screenshots in {1}" -f $captured.Count, $OutputDirectory)
$captured | ForEach-Object { Write-Host "  $_" }
