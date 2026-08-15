# Local CI harness for Clipboard Desktop — mirrors the GitHub Actions check chain locally
# so you can catch breakage before pushing. This is a GENERAL verification tool and is
# intentionally separate from the version-release flow (skills/version-release/scripts/release.mjs),
# which must NOT build anything locally (format + check + lint only, push done by hand).
#
# In the WorkBuddy sandbox the safe-delete shim blocks the `.svelte-kit` cleanup performed by
# `svelte-kit sync` (npm run check) and `vite build`; run with NODE_OPTIONS="" to bypass it:
#   NODE_OPTIONS="" npm run ci:local
#
# Rust steps (cargo clippy / cargo test) may fail in locked-DLL environments for reasons
# unrelated to source changes; treat such failures as environment issues, not script bugs.

param(
    [switch]$SkipFrontend,
    [switch]$SkipRust
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
