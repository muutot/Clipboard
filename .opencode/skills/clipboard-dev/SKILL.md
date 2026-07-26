---
name: clipboard-dev
description: Use when working on the clipboard-desktop project, including TODO audits, feature implementation, settings UI styling, verification, and minimal Git commits. Covers project architecture, evidence requirements, code conventions, build commands, agent isolation, and known pitfalls.
---

# Clipboard Desktop — Development Guide

## Mandatory Maintenance Workflow

Before auditing TODOs, changing settings styles, assigning parallel agents, or committing a feature, read [references/maintenance-workflow.md](references/maintenance-workflow.md) and follow it. Treat `TODO.md`, the current worktree, tests, and rendered/runtime behavior as evidence; never infer completion from intent or from the existence of similarly named code.

## Tech Stack

| Layer            | Technology                           | Notes                                  |
| ---------------- | ------------------------------------ | -------------------------------------- |
| Frontend         | SvelteKit 2 + Svelte 5 (runes)       | SPA mode via adapter-static            |
| Desktop          | Tauri 2                              | No decorations, transparent background |
| Backend          | Rust (edition 2021)                  | `clipboard_desktop_lib` crate          |
| Database         | SQLite (rusqlite, WAL mode)          | Bundled                                |
| Full-text Search | Tantivy 0.26                         | CJK ngram tokenizer                    |
| OCR              | oar-ocr (PpOcr ONNX) / Tesseract CLI | Pluggable engine trait                 |
| Build            | Vite 6                               |                                        |
| Language         | TypeScript + Rust                    |                                        |
| i18n             | Custom, dot-path resolution          | `zh-CN`, `en`                          |

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
│   │   ├── styles/
│   │   │   └── settings-shared.css         # Shared base styles for all settings panels
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

| Layer                        | Storage                                                 | Managed By                |
| ---------------------------- | ------------------------------------------------------- | ------------------------- |
| Frontend (`GeneralSettings`) | Backend config; localStorage is browser/legacy fallback | `settings.ts` store       |
| Backend (`AppConfig`)        | JSON file on disk                                       | Rust `ConfigStore`        |
| Keyboard (`KeyboardConfig`)  | Separate JSON file                                      | Rust `keyboard/config.rs` |

### Search (Outbox Pattern)

Changes are logged to `search_outbox` table, then a synchronizer processes them in batches into the Tantivy index. This decouples clipboard writes from index updates.

### Resource Directory Ownership

- Never run orphan-file cleanup over an arbitrary user-selected directory.
- Claim a custom image/file root only during an explicit settings save, and only when the root is empty or already contains a valid Clipboard ownership marker bound to the current project and resource role.
- Startup must not auto-claim an unmarked directory merely because it is empty. Missing, invalid, foreign, legacy, or unsafe markers keep the directory readable/writable but disable orphan cleanup and require a visible settings warning.
- Reject new image/file roots that are equal, nested, case-equivalent, symlink-equivalent, or overlap application-reserved storage such as the project/data root, database, search index, icons, or configuration paths.
- Cleanup must always skip the ownership marker itself and scan only roots whose ownership was positively validated.

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
- Preserve the main-page visual language unless the task explicitly targets it.
- Before editing settings styles, compare every settings panel and reuse the existing card, heading, description, control, feedback, spacing, and typography patterns.
- Prefer shared CSS variables for settings typography and spacing. Do not introduce a one-off font size when an existing semantic variable fits.
- Do not duplicate shared CSS rules in settings panels. If a rule exists in `src/lib/styles/settings-shared.css`, it does not belong in a panel's `<style>` block. Add panel-specific overrides only when the shared base does not cover the use case.
- Keep setting titles, descriptions, values, controls, and feedback text on one consistent semantic scale across panels.
- Keep primary settings categories in the left navigation. Every category must render a secondary-group row at the top of the right content pane before the setting cards: use compact horizontal tabs when there are multiple secondary sections, and show one current-section item even when there is only one.
- Use one parent settings-shell hierarchy in this order: breadcrumb beginning with `设置 / 一级分组`, then an always-present secondary-group row (multiple items as tabs, a single item as the current section), then one small description line, then the setting cards. The breadcrumb must use the same semantic font size as the description (`--settings-description-size` or its shared equivalent), never the page-title or eyebrow size.
- When the parent shell owns this header, every child settings panel must expose a `showHeader?: boolean` prop and be rendered with `showHeader={false}`. Remove the child header from the DOM; never depend on a parent scoped CSS selector to hide a child component's `.eyebrow` or `h2`, because Svelte component style scoping does not cross component boundaries.

**Form control styling rules (must follow strictly):**

- `<input type="number">` — hide native spin buttons:
  ```css
  input[type="number"] {
    appearance: textfield;
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

**Setting card patterns (use consistently):**

- **Toggle card** — label on left, switch/button on right, same row:

  ```svelte
  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="..." size={17} /></span>
      <div>
        <strong>Label</strong>
        <p>Description</p>
      </div>
    </div>
    <!-- toggle switch, lang-toggle buttons, font-size-input, or theme-select here -->
  ```

  CSS: `.toggle-card { display: flex; align-items: center; justify-content: space-between; gap: 12px; }`

- **Slider card** — label+value on same row (`heading-inline`), slider below, NO wrapper div around `<input>`. If there is a description, nest it inside `<div>` with `<strong>` (description below strong, NOT aligned with icon):

  ```svelte
  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="..." size={17} /></span>
      <div class="heading-inline">
        <!-- Without description: -->
        <strong>Label</strong>
        <!-- With description: -->
        <div>
          <strong>Label</strong>
          <p>Description</p>
        </div>
        <span class="value-label">{value}px</span>
      </div>
    </div>
    <input type="range" class="transparency-slider" oninput={handler} />
  </section>
  ```

  </section>
  ```
  CSS for heading-inline: `display: flex; align-items: center; justify-content: space-between; flex: 1; min-width: 0; gap: 8px;`
  CSS for value-label: `color: #aaa; font-size: 12px; font-variant-numeric: tabular-nums; flex-shrink: 0;`
  CSS for setting-desc: `margin: 2px 0 0; color: #777; font-size: 9.8px;`

- **All range sliders** use class `transparency-slider` — no other slider classes. Do NOT wrap `<input type="range">` in any div.

  ```css
  .transparency-slider {
    width: 100%;
    margin-top: 12px;
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: #2a2a2a;
    outline: none;
    cursor: pointer;
  }
  .transparency-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      #4aa8ff 0%,
      #4aa8ff var(--slider-pct, 50%),
      #2a2a2a var(--slider-pct, 50%),
      #2a2a2a 100%
    );
  }
  .transparency-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border-radius: 50%;
    border: 2px solid #4aa8ff;
    background: #1a1a1a;
    cursor: pointer;
    transition:
      box-shadow 100ms ease,
      transform 100ms ease;
  }
  .transparency-slider::-webkit-slider-thumb:hover {
    box-shadow: 0 0 6px rgba(74, 168, 255, 0.4);
    transform: scale(1.15);
  }
  .transparency-slider::-moz-range-track {
    height: 4px;
    border-radius: 2px;
    background: #2a2a2a;
  }
  .transparency-slider::-moz-range-progress {
    height: 4px;
    border-radius: 2px;
    background: #4aa8ff;
  }
  .transparency-slider::-moz-range-thumb {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid #4aa8ff;
    background: #1a1a1a;
    cursor: pointer;
    transition:
      box-shadow 100ms ease,
      transform 100ms ease;
  }
  .transparency-slider::-moz-range-thumb:hover {
    box-shadow: 0 0 6px rgba(74, 168, 255, 0.4);
    transform: scale(1.15);
  }
  ```

- **Theme colors (must follow strictly):** every color in a component must use a theme CSS variable (`var(--bg-app)`, `var(--bg-settings)`, `var(--accent)`, `var(--text-primary)`, `var(--text-secondary)`, `var(--text-muted)`, `var(--text-faint)`, `var(--border-color)`, `var(--border-subtle)`, `var(--card-bg)`, `var(--surface-bg)`, `var(--statusbar-bg)`, `var(--hover-bg)`, `var(--input-bg)`, `var(--selection-color)`, `var(--success-color)`, `var(--danger-color)`, `var(--warning-color)`, `var(--scrollbar-color)`). Never hardcode hex/rgba values in components — light/custom themes break otherwise. The authoritative variable list lives in `src/lib/utils/theme.ts`; dark defaults live in `DARK_THEME_COLORS` in `src/lib/types/clipboard.ts`. Any raw hex in older snippets in this skill illustrates structure only — always substitute theme variables.

- **Settings typography & metrics (must follow strictly):** settings panels must reference the `--settings-*` semantic variables defined on the settings shell in `StorageSettingsDialog.svelte`, each with its standard fallback:
  | Variable | Fallback | Usage |
  | --- | --- | --- |
  | `--settings-page-title-size` | `18px` | panel `h2` |
  | `--settings-heading-size` | `13px` | `.setting-heading strong` |
  | `--settings-description-size` | `var(--font-size-secondary, 11px)` | descriptions, hints, breadcrumb |
  | `--settings-note-size` | `var(--font-size-tiny, 10px)` | auto-save notes, footnotes |
  | `--settings-control-size` | `var(--font-size-secondary, 11px)` | inputs, selects, buttons, list rows |
  | `--settings-feedback-size` / `--settings-feedback-radius` | description size / `7px` | feedback toast |
  | `--settings-card-radius` | `9px` | `.setting-card` |
  | `--settings-control-radius` | `6px` | inputs, selects, small buttons |
  | `--settings-close-size` / `--settings-close-radius` / `--settings-close-font-size` | `28px` / `7px` / `19px` | close button |

  Never introduce a raw `font-size` or one-off radius in a settings panel when one of these variables fits.

### Settings Panel Shared Styles

All settings panels (General, Compact, Keyboard, FontSize, Theme, IgnoredApps) must import shared base styles via `src/lib/styles/settings-shared.css`. This file is already imported by `src/app.css`.

When creating a new settings panel:
1. Copy the template structure from `GeneralSettingsPanel.svelte` (header + `.settings-scroll` + `.settings-feedback` + `.auto-save-note`)
2. Do NOT redefine shared CSS rules in the panel's `<style>` block — they are already provided by `settings-shared.css`
3. Only add panel-specific styles (e.g., `.lang-toggle` for General, `.font-size-control` for FontSize, `.color-swatch` for Theme)
4. Reference `GeneralSettingsPanel.svelte` as the canonical example

The shared CSS file provides: header, eyebrow, h2, close-button, settings-scroll, setting-card, toggle-card, setting-heading, setting-icon, heading-inline, value-label, toggle-switch, transparency-slider, settings-feedback, auto-save-note, and button cursor.

## Z-Index Layering (Fixed)

| z-index | Element                                  | Context                 |
| ------- | ---------------------------------------- | ----------------------- |
| 51      | `.detail-backdrop`                       | Detail panel backdrop   |
| 52      | `.detail-panel`                          | Detail side panel       |
| 100     | `.image-viewer-overlay`                  | Fullscreen image viewer |
| 101     | `.viewer-close-btn`, `.viewer-zoom-hint` | Viewer controls         |

## Known Pitfalls

See `docs/PITFALLS.md` for detailed bug patterns with code examples. Key categories:

1. **Svelte 5 `$effect`** — signal tracking in conditional branches, `untrack()` usage
2. **`svelte:window` event handlers** — `stopPropagation()` doesn't work across same-target listeners
3. **Tauri multi-window** — separate JS contexts, localStorage-based sync required
4. **Fullscreen state management** — all close paths must go through one function
5. **CSS z-index** — fixed hierarchy, never break the layering
6. **Event delegation** — `setTimeout(0)` for deferred state in click handlers
7. **Settings panel CSS duplication** — Before this refactor, ~200 lines of CSS were duplicated across 5 settings panels. Any style change had to be applied in 4-5 places. After extracting to `src/lib/styles/settings-shared.css`, shared styles are defined once. New panels must NOT copy CSS from existing panels; they must rely on the shared file and only add panel-specific overrides.
