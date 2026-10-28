# Native desktop verification

`TODO.md` section B lists the v1.8 native checks that CI cannot prove. Every
line there is a **runtime or visual evidence gap**, not a missing feature: the
underlying capture, write, broadcast and shutdown paths exist and are covered
by unit/contract tests, but the real OS clipboard, real second window and real
window compositor are outside the scope of `npm run verify`.

This document is the reproducible procedure. Record the observed result next to
the matching checkbox in `TODO.md`; leave the box unchecked until it is actually
observed.

## Ground rules

- Use a disposable application directory. `resolve_project_directory()` uses the
  executable's directory when it is writable, so copying
  `src-tauri/target/debug/clipboard-desktop.exe` into a fresh temp directory
  isolates the database, logs, managed storage and single-instance lock. A
  window opened next to the packaged install shares the real profile instead.
- Native clipboard checks overwrite the OS clipboard. Save `Get-Clipboard` into
  a temp **file** (a shell variable does not survive across tool invocations),
  then restore from that file. Only CF_UNICODETEXT can be restored losslessly;
  other formats are gone for the session.
- Write config JSON without a BOM. `Set-Content -Encoding utf8` writes a BOM on
  Windows PowerShell and the loader then rejects the file as corrupt; use
  `[System.IO.File]::WriteAllText($path, $json)`.
- The GUI subsystem build has no console. Capture output with
  `Start-Process -RedirectStandardOutput/-RedirectStandardError`.
- A running packaged or dev instance owns the OS clipboard and the single-instance
  lock of its own directory. Close it, or accept that the isolated copy cannot
  observe clipboard changes it did not make.

## Item 1 — HTML, files and remote media round-trip

Text round-trip is already recorded (Windows, 2026-10-15). The remaining three
representations:

`scripts/native-clipboard-verify.ps1` automates the capture and copy-back halves
of the HTML and file cases against a real clipboard with an isolated profile. It
writes `HTML Format`, checks the stored `html_content`, writes CF_HDROP, checks
the `file` record plus managed copy, then runs the CLI `copy` and checks the
clipboard formats. The isolated database lives at
`storage\database\clipboard.sqlite3`, not `history.sqlite` in the executable
directory. The parts it cannot script — WebView "format paste", Explorer paste,
and remote media — stay manual below.

### HTML

Capture side: a producer writes CF_HTML (`"HTML Format"`) to the clipboard. The
isolated app must store the fragment in `html_content` unchanged. Read path:
`src-tauri/src/platform/windows/html.rs`. Write path for a formatted paste:
`writeClipboardHtml` in `src/lib/services/clipboard.ts` through the WebView's
`navigator.clipboard.write`, which registers CF_HTML/
`Rich Text Format` — the backend only owns plain text and CF_HDROP, so a CLI
process cannot reproduce the formatted write.

1. Set a CF_HTML payload containing a unique fragment (`<b>marker-<guid></b>`).
2. Wait for the capture worker, then read the record:
   `select html_content from clipboard_items where html_content is not null`
   via `python -c "import sqlite3, ..."` against the isolated `clipboard.sqlite3`.
3. Assert the stored fragment equals the marker.
4. In the GUI, use the card's **format paste** action (not the plain copy) and
   paste into a rich-text target (WordPad/Word); the bold marker must survive.
   A plain copy intentionally writes text only; that is the designed behaviour,
   not a regression.

### Files

Capture side: CF_HDROP of a real temp file must produce a `file` record whose
managed copy exists. Write side: `write_clipboard_files_with_self_trigger`
rebuilds CF_HDROP so Explorer paste copies the source file.

1. Create `C:\...\marker-<guid>.txt`, put it on the clipboard as CF_HDROP.
2. Assert a `file` record appears and its `resource_path` file exists under the
   isolated managed storage.
3. `clipboard-desktop.exe copy --query <id>` then inspect the clipboard formats
   (CF_HDROP must be present). Paste in Explorer and confirm the file content
   matches.

### Remote media

A record synced from another device has a remote resource and no local file.
`materialize_clipboard_item` (`src-tauri/src/commands/sync/mod.rs`) downloads it
on copy/preview.

1. Point a second device at the same S3 bucket, sync a file/image, then delete
   the local managed copy on the first device.
2. Copy the item; assert the download runs and the local bytes match the
   original (SHA-256).

## Item 2 — Two windows: settings and item state

Settings: `set_general_settings` merges a patch against the **current** config
under the config lock before writing, and broadcasts `general-settings-changed`
(`src-tauri/src/commands/config/settings.rs`); the frontend re-applies it
(`src/lib/services/settings.ts`). Two windows therefore converge, and unrelated
fields edited concurrently do not clobber each other.

Item state: 11 item commands carry `AppHandle` and broadcast
`clipboard-items-changed`, folded by `src/lib/utils/store/item-changes.ts`.

1. Open the main window and the float window side by side on the same isolated
   profile.
2. Change a general setting in one; assert the other reflects it without a manual
   reload.
3. Change a different general field in the second window before the first
   broadcast arrives; assert both edits survive.
4. Favorite/copy/delete in one window; assert the other's row updates live.
   Screenshot each step into the run directory.

## Item 3 — Exit during tags, restore and a slow S3 transfer

`RunEvent::ExitRequested` calls `stop_runtime_services`
(`src-tauri/src/shutdown.rs`), which cancels the sync token, then
`BackgroundOperations::shutdown` cancels and waits up to 30 s for in-flight tag/
backup operations before stopping every worker.

1. Tag apply: start `apply_auto_tag_history` on a large history, exit the app
   mid-run, relaunch, and assert the row count/tags are consistent (no partial
   transaction, no duplicate).
2. Restore: start a backup restore, exit mid-run, relaunch, and assert the
   import either fully committed or rolled back, with the quarantine snapshot
   still present.
3. Slow S3: run `scripts/sync-s3-test.ps1` for the transport contract, then use a
   throttled endpoint so a snapshot/segment PUT is in flight, exit the app, and
   assert the published head did not advance and pending work survives a
   restart-then-retry convergence.

## Item 4 — Theme and width visual matrix

Capture the **real** window, not browser fixtures. Structural/type checks do not
prove appearance.

`scripts/theme-screenshot-matrix.ps1` drives this: it launches an isolated copy
per scenario, seeds `themeColors` (not just the `theme` selector — the applied
palette comes from `themeColors`), seeds rows straight into the isolated SQLite
database, and captures with `scripts/capture-window.ps1` (Win32 + GDI+
`PrintWindow(PW_RENDERFULLCONTENT)`, with a screen-rectangle fallback when the
bitmap comes back flat).

### Width contract

The main window enforces a **730 px minimum** (`tauri.conf.json` `minWidth`, the
backend `MAIN_WINDOW_MIN_WIDTH`, and `geometry.rs`). A native main window
therefore cannot reach 520 px. The 520 px responsive breakpoint lives in
`DetailPanel.svelte` (`width: min(520px, 100vw)`, `@media (max-width: 520px)`)
and in the browser fixtures — not in a resizable native main window. Forcing the
window below its minimum clips the layout (`+page.svelte` `.app-shell {
min-width: 710px }` keeps the shell wide), which is out of contract, not a
regression.

The real narrow native surface is the 320 px float panel
(`commands/float.rs`, `FLOAT_WIDTH = 320`). Capture that for the narrow theme
rows.

### Procedure

1. Capture `dark`, `light` and a `custom` palette at the normal size and at the
   enforced 730 px minimum.
2. Capture the 320 px float panel per theme.
3. Include the text-transparency toggle combined with `acrylic`/`mica` on the
   main window.
4. Assert readability: no clipped controls, no overlapping text, and legible
   foreground over the translucent layer.

`PrintWindow` renders the window's own content, not the desktop behind it, so it
cannot prove the _translucency_ itself. Judge that from an on-screen capture of
the window over a controlled backdrop. A washed-out `light` + `mica` +
text-transparency-at-85% frame was observed but is not yet attributed between
real appearance and the capture method.
