---
name: clipboard-dev
description: Use when working on the clipboard-desktop project, including TODO audits, feature implementation, settings UI styling, verification, and minimal Git commits. Covers project architecture, evidence requirements, code conventions, build commands, agent isolation, and known pitfalls.
---

# Clipboard Desktop — Development Guide

## Mandatory Maintenance Workflow

Before auditing TODOs, changing settings styles, assigning parallel agents, or committing a feature, read [references/maintenance-workflow.md](references/maintenance-workflow.md) and follow it. Treat `TODO.md`, the current worktree, tests, and rendered/runtime behavior as evidence; never infer completion from intent or from the existence of similarly named code.

## Skill Update Check (Every Commit)

**Before every commit**, evaluate whether this skill file needs updating:

1. Did I add/remove/rename a component, service, util, route, or backend module? → update the project structure tree
2. Did I change a public API (Tauri command signature, frontend service function, TypeScript interface)? → update the relevant section
3. Did I introduce a new pattern or convention? → add it to the appropriate section (Code Conventions, Architecture Patterns, etc.)
4. Did I change CSS variables, theming, or settings styles? → update the CSS section
5. Did I add/modify a settings field? → update the GeneralSettings reference

If any answer is yes, update this skill file in the same commit. A stale skill causes repeated mistakes.

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

See [references/project-structure.md](references/project-structure.md) for the full file listing (components, services, utils, routes, backend modules). See also:
- [references/components.md](references/components.md) — component props, state, patterns
- [references/services.md](references/services.md) — service exports, signatures, patterns
- [references/css-theming.md](references/css-theming.md) — CSS variables, theme system, shared classes
- [references/backend-architecture.md](references/backend-architecture.md) — database pool, recovery, search, content modules
- [references/settings-panels.md](references/settings-panels.md) — panel conventions, state management, feedback pattern

```
clipboard/
├── src/                              # Frontend (Svelte + TypeScript)
│   ├── routes/                       # +page.svelte (main), settings/, viewer/
│   ├── lib/
│   │   ├── components/               # 15 Svelte components
│   │   ├── services/                 # 10 service modules
│   │   ├── utils/                    # 5 utility modules
│   │   ├── types/                    # clipboard.ts, memory.ts
│   │   ├── i18n/                     # locales/ (zh-CN, en)
│   │   ├── styles/                   # settings-shared.css
│   │   └── data/                     # demo-items.ts
│   ├── app.css                       # Global styles, CSS variables, theme
│   └── app.html                      # HTML shell
│
├── src-tauri/                        # Backend (Rust)
│   ├── tauri.conf.json               # Tauri v2 config
│   ├── Cargo.toml
│   ├── capabilities/                 # Tauri v2 permissions
│   └── src/
│       ├── main.rs / lib.rs / config.rs / memory.rs
│       ├── domain/                   # ClipboardItem, OcrResult
│       ├── storage/                  # repository, pool, recovery, migrations
│       ├── search/                   # Tantivy index, sync, query, schema
│       ├── ocr/                      # engine, worker, ppocr, tesseract, models
│       ├── keyboard/                 # binding, config, manager, matcher
│       ├── content/                  # detector, thumbnail, hash, file_store, actions, transform
│       ├── platform/                 # windows, macos, linux_x11, linux_wayland
│       ├── export/                   # JSON/CSV/plain-text export & import
│       ├── privacy/                  # Privacy manager
│       ├── performance/              # Performance tracking, memory monitor
│       └── cli/                      # CLI + loopback HTTP API server
│
├── docs/                             # PITFALLS.md, OCR.md, SEARCH.md, DEFAULTS_AND_PRIVACY.md
└── .opencode/skills/clipboard-dev/   # This skill
```

## Build & Verify

```bash
# Development
npm run dev                    # Vite dev server (port 1420)
npm run tauri dev              # Full Tauri dev (backend + frontend)

# Type checking (run after every change)
npm run check                  # svelte-kit sync + svelte-check
npm run check:watch            # Continuous type checking

# Production build
npm run build                  # Vite build → ../build
npm run preview                # Preview production build locally

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

Key interfaces in `src/lib/types/clipboard.ts`. See [references/settings-reference.md](references/settings-reference.md) for the full `GeneralSettings` field listing with defaults.

- `ClipboardItem` — displayed item with kind, title, preview, sourceApp, etc.
- `PersistedClipboardItem` — raw from backend via Tauri invoke
- `GeneralSettings` — 36+ fields: language, theme, display, fontSizes, compact mode, search settings, etc.
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

### Dual-Window Architecture

The app uses two Tauri WebviewWindows:
- **Main window** — clipboard list, settings, detail panel
- **Viewer window** (`viewer/+page.svelte`) — fullscreen image viewing (desktop mode). Listens for `viewer:open` events.

### Settings Bootstrap

`settings-bootstrap.ts` applies `GeneralSettings` to the document root on startup:
- `applyGeneralSettingsToDocument()` — sets CSS custom properties for font sizes, display, theme
- `syncCompactShellClass()` — toggles `.compact` class on `.app-shell`

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

Settings panels follow a `showHeader?: boolean` prop convention — when rendered inside `StorageSettingsDialog`, child panels pass `showHeader={false}` to hide their own header. See [references/settings-reference.md](references/settings-reference.md) for all fields.

### Search (Outbox Pattern)

Changes are logged to `search_outbox` table, then a synchronizer processes them in batches into the Tantivy index. This decouples clipboard writes from index updates.

### Search Result Cache

See [references/search-cache-strategy.md](references/search-cache-strategy.md) for cache invalidation rules.

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

- **All range sliders** use class `transparency-slider` — no other slider classes. Do NOT wrap `<input type="range">` in any div. The actual themed CSS lives in `src/lib/styles/settings-shared.css`; use CSS variables, never hardcoded hex:

  ```css
  .transparency-slider {
    width: 100%;
    margin-top: 12px;
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    border-radius: 2px;
    background: var(--hover-bg);
    outline: none;
    cursor: pointer;
  }
  .transparency-slider::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      var(--selection-color) 0%,
      var(--selection-color) var(--slider-pct, 50%),
      var(--hover-bg) var(--slider-pct, 50%),
      var(--hover-bg) 100%
    );
  }
  .transparency-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border-radius: 50%;
    border: 2px solid var(--selection-color);
    background: var(--input-bg);
    cursor: pointer;
    transition: box-shadow 100ms ease, transform 100ms ease;
  }
  .transparency-slider::-webkit-slider-thumb:hover {
    box-shadow: 0 0 6px color-mix(in srgb, var(--selection-color) 40%, transparent);
    transform: scale(1.15);
  }
  /* Firefox track, progress, thumb follow the same variable pattern */
  ```

- **Theme colors (must follow strictly):** every color in a component must use a theme CSS variable (`var(--bg-app)`, `var(--bg-settings)`, `var(--accent)`, `var(--text-primary)`, `var(--text-secondary)`, `var(--text-muted)`, `var(--text-faint)`, `var(--border-color)`, `var(--border-subtle)`, `var(--card-bg)`, `var(--surface-bg)`, `var(--statusbar-bg)`, `var(--hover-bg)`, `var(--input-bg)`, `var(--selection-color)`, `var(--success-color)`, `var(--danger-color)`, `var(--warning-color)`, `var(--scrollbar-color)`). Never hardcode hex/rgba values in components — light/custom themes break otherwise. The authoritative variable list lives in `src/lib/utils/theme.ts`; dark defaults live in `DARK_THEME_COLORS` in `src/lib/types/clipboard.ts`. Any raw hex in older snippets in this skill illustrates structure only — always substitute theme variables.

- **Settings typography & metrics (must follow strictly):** settings panels must reference the `--settings-*` semantic variables defined on the settings shell in `StorageSettingsDialog.svelte`, each with its standard fallback:

  | Variable                                                                           | Fallback                           | Usage                               |
  | ---------------------------------------------------------------------------------- | ---------------------------------- | ----------------------------------- |
  | `--settings-page-title-size`                                                       | `18px`                             | panel `h2`                          |
  | `--settings-heading-size`                                                          | `13px`                             | `.setting-heading strong`           |
  | `--settings-description-size`                                                      | `var(--font-size-secondary, 11px)` | descriptions, hints, breadcrumb     |
  | `--settings-note-size`                                                             | `var(--font-size-tiny, 10px)`      | auto-save notes, footnotes          |
  | `--settings-control-size`                                                          | `var(--font-size-secondary, 11px)` | inputs, selects, buttons, list rows |
  | `--settings-feedback-size` / `--settings-feedback-radius`                          | description size / `7px`           | feedback toast                      |
  | `--settings-card-radius`                                                           | `9px`                              | `.setting-card`                     |
  | `--settings-control-radius`                                                        | `6px`                              | inputs, selects, small buttons      |
  | `--settings-close-size` / `--settings-close-radius` / `--settings-close-font-size` | `28px` / `7px` / `19px`            | close button                        |
  | `--settings-icon-radius`                                                           | `7px`                              | `.setting-icon` border-radius       |

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
