---
name: clipboard-dev
description: Use when working on the clipboard-desktop project. Covers development workflow, project architecture, code conventions, build commands, and references to known pitfalls.
---

# Clipboard Desktop — Development Guide

## Tech Stack

| Layer | Technology | Notes |
|-------|-----------|-------|
| Frontend | SvelteKit 2 + Svelte 5 (runes) | SPA mode via adapter-static |
| Desktop | Tauri 2 | No decorations, transparent background |
| Backend | Rust (edition 2021) | `clipboard_desktop_lib` crate |
| Database | SQLite (rusqlite, WAL mode) | Bundled |
| Full-text Search | Tantivy 0.26 | CJK ngram tokenizer |
| OCR | oar-ocr (PpOcr ONNX) / Tesseract CLI | Pluggable engine trait |
| Build | Vite 6 | |
| Language | TypeScript + Rust | |
| i18n | Custom, dot-path resolution | `zh-CN`, `en` |

## Project Structure

```
clipboard/
├── src/                              # Frontend (Svelte + TypeScript)
│   ├── routes/
│   │   ├── +page.svelte              # Main page: clipboard list, selection, virtual scroll
│   │   ├── +layout.svelte            # Root layout, global state init
│   │   ├── +layout.ts                # SPA config (prerender disabled)
│   │   └── settings/
│   │       └── +page.svelte          # Settings window (separate Tauri WebviewWindow)
│   ├── lib/
│   │   ├── components/
│   │   │   ├── DetailPanel.svelte        # Side panel + fullscreen image viewer
│   │   │   ├── ClipboardCard.svelte      # Item card in list
│   │   │   ├── StorageSettingsDialog.svelte  # Settings dialog (tabs: General/Storage/Keyboard/IgnoredApps)
│   │   │   ├── GeneralSettingsPanel.svelte   # UI prefs (theme, language, fullscreen mode)
│   │   │   ├── KeyboardSettingsPanel.svelte  # Shortcut config UI
│   │   │   ├── IgnoredAppsSettingsPanel.svelte
│   │   │   ├── AppIcon.svelte            # Source app icon display
│   │   │   ├── CodePreview.svelte        # Syntax highlighting (highlight.js)
│   │   │   ├── MarkdownPreview.svelte    # Markdown rendering
│   │   │   └── Toast.svelte              # Toast notification system
│   │   ├── services/
│   │   │   ├── settings.ts           # GeneralSettings store (localStorage, cross-window sync)
│   │   │   ├── clipboard.ts          # Data fetching, mapping PersistedClipboardItem → ClipboardItem
│   │   │   ├── toast.ts              # Toast notification store
│   │   │   ├── runtime.ts            # isTauriRuntime() detection
│   │   │   ├── capture.ts            # Clipboard capture service
│   │   │   ├── keyboard.ts           # Frontend keyboard shortcut handling
│   │   │   ├── paths.ts              # Path utilities
│   │   │   └── storage.ts            # Storage-related service
│   │   ├── types/
│   │   │   └── clipboard.ts          # All TypeScript interfaces
│   │   ├── i18n/
│   │   │   ├── index.ts              # Locale detection, resolvePath(), t()
│   │   │   ├── types.ts              # Translation key types
│   │   │   └── locales/
│   │   │       ├── en.ts
│   │   │       └── zh-CN.ts
│   │   ├── utils/
│   │   │   ├── virtual-scroll.ts     # Custom virtual scrolling
│   │   │   ├── time.ts               # formatRelativeTime()
│   │   │   └── date-query.ts         # Natural language date parsing
│   │   └── data/                     # Static data
│   ├── app.css                       # Global styles, CSS variables, theme
│   └── app.html                      # HTML shell
│
├── src-tauri/                        # Backend (Rust)
│   ├── tauri.conf.json               # Tauri v2 config (windows, security, bundle)
│   ├── Cargo.toml                    # Rust dependencies
│   ├── capabilities/                 # Tauri v2 permissions
│   └── src/
│       ├── main.rs                   # Entry point
│       ├── lib.rs                    # Tauri builder, 40+ #[tauri::command], clipboard monitor thread
│       ├── config.rs                 # AppConfig, StorageConfig, OcrConfig, etc.
│       ├── domain/
│       │   ├── clipboard_item.rs     # ClipboardItem, ClipboardKind enums
│       │   └── ocr_result.rs         # OcrResult, OcrStatus
│       ├── storage/
│       │   ├── migrations.rs         # SQLite schema (clipboard_items, ocr_results, search_outbox)
│       │   ├── repository.rs         # ClipboardRepository (CRUD)
│       │   ├── ocr_repository.rs     # OcrRepository
│       │   ├── search_repository.rs  # Search-specific queries
│       │   ├── database.rs           # Database init, connection pool
│       │   ├── paths.rs              # StoragePaths (data dir, icons, files, images)
│       │   └── error.rs              # StorageError enum
│       ├── search/
│       │   ├── index.rs              # SearchIndex (Tantivy writer/reader)
│       │   ├── sync.rs               # SearchSynchronizer (outbox pattern)
│       │   ├── query.rs              # Query building, CJK support
│       │   ├── schema.rs             # Tantivy field definitions
│       │   └── manifest.rs           # Field boosting config
│       ├── ocr/
│       │   ├── engine.rs             # OcrEngine trait
│       │   ├── worker.rs             # Background OCR worker thread
│       │   ├── ppocr.rs              # PaddleOCR ONNX implementation
│       │   ├── tesseract.rs          # Tesseract CLI wrapper
│       │   └── noop.rs               # No-op fallback
│       ├── keyboard/
│       │   ├── binding.rs            # ShortcutBinding parser (chords, double-modifier)
│       │   ├── config.rs             # KeyboardConfig (JSON file on disk)
│       │   ├── manager.rs            # HotkeyManager (Windows global hotkeys)
│       │   └── matcher.rs            # ShortcutMatcher
│       ├── content/
│       │   ├── detector.rs           # Content type detection (text/link/image/file)
│       │   └── thumbnail.rs          # Image thumbnail generation (JPEG, max 400px)
│       ├── platform/
│       │   ├── windows_clipboard.rs  # Windows clipboard monitoring
│       │   ├── windows_hotkey.rs     # Windows global hotkey registration
│       │   └── ...                   # Platform abstractions
│       ├── export/                   # JSON/CSV/plain text export & import
│       ├── privacy/                  # Privacy manager (pause/resume, app ignore)
│       ├── performance/              # Performance tracking, startup metrics
│       └── cli/                      # CLI argument parsing
│
├── docs/
│   ├── PITFALLS.md                   # Known bugs and gotchas
│   ├── OCR.md
│   ├── SEARCH.md
│   └── DEFAULTS_AND_PRIVACY.md
│
└── .opencode/
    └── skills/clipboard-dev/
        └── SKILL.md                  # This file
```

## Build & Verify

```bash
# Development
npm run dev                    # Vite dev server (port 1420)

# Type checking (run after every change)
npm run check                  # svelte-kit sync + svelte-check

# Production build
npm run build                  # Vite build → ../build

# Full verification (CI-grade)
npm run verify                 # format:check + check + build + test:rust + lint:rust

# Individual checks
npm run format:check           # Prettier + cargo fmt
npm run test:rust              # cargo test
npm run lint:rust              # cargo clippy -D warnings
npm run format                 # Auto-fix formatting
```

## Data Model

### Database Schema (SQLite)

```sql
-- Core clipboard history
clipboard_items (
  id TEXT PK,
  kind TEXT CHECK (kind IN ('text','link','image','file')),
  title TEXT,
  text_content TEXT,
  resource_path TEXT,         -- file path for image/file content
  preview_path TEXT,          -- thumbnail path
  content_hash TEXT,          -- dedup key (UNIQUE with kind)
  source_app TEXT,            -- originating application name
  icon_path TEXT,             -- cached app icon path
  size_bytes INTEGER,
  created_at_ms INTEGER,
  last_used_at_ms INTEGER,
  is_favorite INTEGER DEFAULT 0,
  deleted INTEGER DEFAULT 0,  -- soft delete (recycle bin)
  deleted_at_ms INTEGER,
  metadata_json TEXT DEFAULT '{}',
  UNIQUE (kind, content_hash)
)

-- OCR results (1:1 with image items)
ocr_results (
  item_id TEXT PK FK → clipboard_items,
  status TEXT CHECK (status IN ('pending','processing','completed','failed')),
  engine TEXT,
  full_text TEXT,
  blocks_json TEXT,
  image_hash TEXT,
  ...
)

-- Search index sync queue (outbox pattern)
search_outbox (
  sequence INTEGER PK AUTOINCREMENT,
  item_id TEXT,
  operation TEXT CHECK (operation IN ('upsert','delete')),
  ...
)
```

### Frontend Types

Key interfaces in `src/lib/types/clipboard.ts`:
- `ClipboardItem` — displayed item with kind, title, preview, sourceApp, etc.
- `PersistedClipboardItem` — raw from backend via Tauri invoke
- `GeneralSettings` — frontend settings (language, fontSize, theme, imageFullscreenMode, etc.)
- `CaptureSettings` — backend storage settings

### Rust Domain Types

In `src-tauri/src/domain/`:
- `ClipboardItem` — `#[serde(rename_all = "camelCase")]`, maps 1:1 to DB row
- `ClipboardKind` — enum: `Text`, `Link`, `Image`, `File` (serialized as lowercase)
- `OcrResult` — OCR output with status, text, blocks

## Architecture Patterns

### Frontend ↔ Backend IPC

All communication uses Tauri's `invoke()` (frontend) and `#[tauri::command]` (Rust). 40+ commands in `lib.rs`.

```typescript
// Frontend
const items = await invoke<ClipboardItem[]>("get_clipboard_items", { offset: 0, limit: 50 });

// Rust
#[tauri::command]
fn get_clipboard_items(offset: u64, limit: u64) -> Result<Vec<ClipboardItem>, String> { ... }
```

### Event-Driven Updates

Backend emits events via `app.emit("event-name", payload)`. Frontend listens:

```typescript
listen<PersistedClipboardItem>("clipboard-item-added", (event) => { ... });
```

### State Management

- **Component state**: Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`)
- **Cross-component services**: Svelte writable stores (`settings`, `toast`, `clipboard`)
- **Backend state**: Rust `Mutex<T>` and `Arc<T>` for shared state

### Settings (Two Layers)

| Layer | Storage | Managed By |
|-------|---------|-----------|
| Frontend (`GeneralSettings`) | localStorage | `settings.ts` store |
| Backend (`AppConfig`) | JSON file on disk | Rust `ConfigStore` |
| Keyboard (`KeyboardConfig`) | Separate JSON file | Rust `keyboard/config.rs` |

### Search (Outbox Pattern)

Changes are logged to `search_outbox` table, then a synchronizer processes them in batches into the Tantivy index. This decouples clipboard writes from index updates.

### OCR (Worker Pattern)

A background worker thread processes OCR jobs sequentially via a message queue. The `OcrEngine` trait allows pluggable engines (PpOcr, Tesseract, Noop).

### Virtual Scrolling

Custom implementation in `virtual-scroll.ts` handles large clipboard lists. Items have dynamic heights (text vs image cards).

## Code Conventions

### Svelte 5

- Props: `interface Props { ... }` then `let { item, onclose }: Props = $props()`
- State: `let x = $state(initialValue)`
- Derived: `let x = $derived(expression)`
- Effects: `$effect(() => { ... })` — use `untrack()` to avoid unwanted dependencies
- No comments unless requested

### TypeScript

- Strict mode via tsconfig
- Interfaces in `src/lib/types/clipboard.ts`
- Services return `Result`-like patterns or throw

### Rust

- `#[serde(rename_all = "camelCase")]` on all structs (JSON matches frontend convention)
- `#[serde(default)]` on structs for forward compatibility
- `BTreeMap<String, Value>` extra fields for unknown keys
- `Result<T, String>` for Tauri commands (String error → frontend)
- Module-per-concern: `storage/`, `search/`, `ocr/`, `keyboard/`, etc.

### i18n

- Keys: dot-path notation (`"detail.fullscreenPreview"`, `"general.desktopFullscreen"`)
- Usage: `const _t = (path, params?) => resolvePath($messages, path, params)`
- Add keys to BOTH `en.ts` and `zh-CN.ts` + `types.ts`

### CSS

- Scoped `<style>` blocks in Svelte components
- CSS custom properties for theming in `app.css`
- BEM-like naming (`.detail-panel`, `.viewer-close-btn`)
- Glass morphism: `backdrop-filter: blur()`, semi-transparent backgrounds

**Form control styling rules (must follow strictly):**

- `<input type="number">` — hide native spin buttons:
  ```css
  input[type="number"] {
    -moz-appearance: textfield;
  }
  input[type="number"]::-webkit-inner-spin-button,
  input[type="number"]::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  ```
- `<select>` — hide native dropdown arrow:
  ```css
  select {
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
  }
  ```

## Z-Index Layering (Fixed)

| z-index | Element | Context |
|---------|---------|---------|
| 51 | `.detail-backdrop` | Detail panel backdrop |
| 52 | `.detail-panel` | Detail side panel |
| 100 | `.image-viewer-overlay` | Fullscreen image viewer |
| 101 | `.viewer-close-btn`, `.viewer-zoom-hint` | Viewer controls |

## Known Pitfalls

See `docs/PITFALLS.md` for detailed bug patterns with code examples. Key categories:

1. **Svelte 5 `$effect`** — signal tracking in conditional branches, `untrack()` usage
2. **`svelte:window` event handlers** — `stopPropagation()` doesn't work across same-target listeners
3. **Tauri multi-window** — separate JS contexts, localStorage-based sync required
4. **Fullscreen state management** — all close paths must go through one function
5. **CSS z-index** — fixed hierarchy, never break the layering
6. **Event delegation** — `setTimeout(0)` for deferred state in click handlers
