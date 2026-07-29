param(
    [switch]$SkipFrontend,
    [switch]$SkipRust,
    [switch]$Fast
)

$ErrorActionPreference = "Stop"
$rootDir = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$rustDir = Join-Path $rootDir "src-tauri"
$startTime = Get-Date

function Step($name, $scriptBlock) {
    Write-Host "`n========================================" -ForegroundColor Cyan
    Write-Host "  $name" -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan
    $stepStart = Get-Date
    & $scriptBlock
    if ($LASTEXITCODE -ne 0) {
        Write-Host "  FAIL (exit $LASTEXITCODE)" -ForegroundColor Red
        exit 1
    }
    $elapsed = (Get-Date) - $stepStart
    Write-Host "  PASS ($($elapsed.TotalSeconds.ToString('0.0'))s)" -ForegroundColor Green
}

Set-Location $rootDir

Write-Host "========================================" -ForegroundColor Magenta
Write-Host "  Local CI (Clipboard Desktop)" -ForegroundColor Magenta
Write-Host "  Platform: Windows" -ForegroundColor Magenta
Write-Host "========================================" -ForegroundColor Magenta

if (-not $SkipFrontend) {
    Step "Frontend: npm ci" { & "npm.cmd" ci }
    Step "Frontend: format:check" { & "npm.cmd" run format:check }
    Step "Frontend: check (svelte-check)" { & "npm.cmd" run check }
    Step "Frontend: build (vite)" { & "npm.cmd" run build }
}

if (-not $SkipRust) {
    Step "Rust: cargo fmt --check" {
        cargo fmt --manifest-path "$rustDir\Cargo.toml" -- --check
    }
    Step "Rust: cargo clippy" {
        cargo clippy -j 1 --manifest-path "$rustDir\Cargo.toml" --all-targets -- -D warnings
    }
    Step "Rust: cargo test" {
        cargo test -j 1 --manifest-path "$rustDir\Cargo.toml"
    }
}

$total = (Get-Date) - $startTime
Write-Host "`n========================================" -ForegroundColor Green
Write-Host "  ALL CI PASSED ($($total.TotalSeconds.ToString('0.0'))s)" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Green
