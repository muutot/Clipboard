# Project Structure — Detailed File Listing

## Frontend Components (`src/lib/components/`)

| File | Description |
|---|---|
| `DetailPanel.svelte` | Side panel + fullscreen image viewer |
| `ClipboardCard.svelte` | Item card in list |
| `StorageSettingsDialog.svelte` | Settings dialog (tabs: General/Storage/Keyboard/IgnoredApps) |
| `GeneralSettingsPanel.svelte` | UI prefs (theme, language, fullscreen mode) |
| `KeyboardSettingsPanel.svelte` | Shortcut config UI |
| `IgnoredAppsSettingsPanel.svelte` | Ignored apps management |
| `FontSizeSettingsPanel.svelte` | Font-size tuning UI with subnav (interface/card), sliders + number inputs |
| `ThemeSettingsPanel.svelte` | Theme mode selector (dark/light/custom), color swatches, custom preset CRUD |
| `CompactSettingsPanel.svelte` | Compact mode toggle + 9 dimension sliders |
| `ContextMenu.svelte` | Right-click context menu with keyboard-dismiss, outside-click, viewport-bound positioning |
| `CodeEditor.svelte` | Split-pane code editor with contenteditable, line numbers, live preview |
| `AppIcon.svelte` | Source app icon display |
| `CodePreview.svelte` | Syntax highlighting (highlight.js) |
| `MarkdownPreview.svelte` | Markdown rendering |
| `Toast.svelte` | Toast notification system |

## Frontend Services (`src/lib/services/`)

| File | Description |
|---|---|
| `settings.ts` | GeneralSettings store (localStorage, cross-window sync) |
| `clipboard.ts` | Data fetching, mapping PersistedClipboardItem → ClipboardItem |
| `toast.ts` | Toast notification store |
| `runtime.ts` | isTauriRuntime() detection |
| `capture.ts` | Clipboard capture service |
| `keyboard.ts` | Frontend keyboard shortcut handling |
| `paths.ts` | Path utilities |
| `storage.ts` | Storage-related service (StorageStatus, StorageConfig, repair, search sync, etc.) |
| `memory.ts` | getMemoryDiagnostics() — reads process-group memory snapshot from backend |
| `settings-bootstrap.ts` | applyGeneralSettingsToDocument() — sets CSS custom properties on document root |

## Frontend Utils (`src/lib/utils/`)

| File | Description |
|---|---|
| `virtual-scroll.ts` | Custom virtual scrolling |
| `time.ts` | formatRelativeTime() |
| `date-query.ts` | Natural language date parsing |
| `theme.ts` | applyThemeColors() — sets 20 CSS custom properties from ThemeColors |
| `keyboard.ts` | isEditableKeyboardTarget() — checks if target is editable element |

## Frontend Types (`src/lib/types/`)

| File | Description |
|---|---|
| `clipboard.ts` | All TypeScript interfaces (ClipboardItem, GeneralSettings, etc.) |
| `memory.ts` | MemoryDiagnostics, MemoryProcess, SystemMemory interfaces |

## Frontend Routes (`src/routes/`)

| File | Description |
|---|---|
| `+page.svelte` | Main page: clipboard list, selection, virtual scroll, search pagination |
| `+layout.svelte` | Root layout, global state init |
| `+layout.ts` | SPA config (prerender disabled) |
| `settings/+page.svelte` | Settings window (separate Tauri WebviewWindow) |
| `viewer/+page.svelte` | Dedicated WebviewWindow for fullscreen image viewing (desktop mode) |

## Backend Modules (`src-tauri/src/`)

| Module | Files | Description |
|---|---|---|
| top-level | `main.rs`, `lib.rs`, `config.rs`, `memory.rs` | Entry point, 40+ Tauri commands, config, memory diagnostics |
| `domain/` | `clipboard_item.rs`, `ocr_result.rs` | Domain types (ClipboardItem, ClipboardKind, OcrResult) |
| `storage/` | `repository.rs`, `ocr_repository.rs`, `search_repository.rs`, `database.rs`, `paths.rs`, `error.rs`, `migrations.rs`, `pool.rs`, `recovery.rs` | CRUD, connection pooling (round-robin reads), corruption recovery/backup |
| `search/` | `index.rs`, `sync.rs`, `query.rs`, `schema.rs`, `manifest.rs`, `error.rs` | Tantivy index, outbox sync, CJK ngram, search cache |
| `ocr/` | `engine.rs`, `worker.rs`, `ppocr.rs`, `tesseract.rs`, `noop.rs`, `models.rs` | Pluggable OCR engine, model spec definitions |
| `keyboard/` | `binding.rs`, `config.rs`, `manager.rs`, `matcher.rs` | Shortcut parsing, global hotkeys, chord matching |
| `content/` | `detector.rs`, `thumbnail.rs`, `transform.rs`, `resource_metadata.rs`, `hash.rs`, `file_store.rs`, `actions.rs` | Content detection, thumbnails, text transforms, MIME types, dedup hashing, file storage, quick actions |
| `platform/` | `windows_clipboard.rs`, `windows_hotkey.rs`, `macos.rs`, `linux_x11.rs`, `linux_wayland.rs` | Platform-specific clipboard and hotkey implementations |
| `export/` | `mod.rs` | JSON/CSV/plain-text export & import |
| `privacy/` | `mod.rs` | Privacy manager (pause/resume, app ignore, sensitive pattern detection) |
| `performance/` | `mod.rs` | Performance tracking, startup metrics, search latency, memory monitor |
| `cli/` | `mod.rs`, `api.rs` | CLI argument parsing, loopback HTTP API server for scripts/automation |
