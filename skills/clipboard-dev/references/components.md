# Frontend Components

Read each component's current `Props` interface before changing a call site. This reference records ownership and invariants, not an exhaustive signature snapshot.

## Main workflow components

### `ClipboardCard.svelte`

Owns one list item: text/link/image/file rendering, source metadata, quick/content actions, context-menu actions, inline editing, favorite/delete/restore/copy/detail/plain-paste/format-paste/clean-paste callbacks, compact layout, and measurement reporting.

Key contracts:

- The route owns collection state and persistence decisions; the card emits controlled callbacks keyed by item ID.
- `onheightchange` plus `heightMeasurementKey` feed virtual scrolling. Re-measure whenever visible content, compact metrics, action/meta visibility, title state, or line limits can alter height.
- Resource previews use `convertFileSrc`; source app icons are resolved from the managed `iconsDir` store using the icon key, not an arbitrary full path.
- Card action order and context-menu behavior must stay aligned. Reuse the same callback path rather than creating a second implementation.
- Compact dimensions come from `GeneralSettings` and must remain aligned with `virtual-scroll.ts` and route height calculations.
- Tag chips share the title/file text line (right-aligned, `flex: 0 0 auto`), so adding tags does **not** add a new row and must not change card height. Right-click `Add tag` toggles an inline input; a chip's `×` removes the tag via `onsavetags`; clicking a chip calls `ontoggleTagFilter`. Keep tag height changes out of `estimatedCardHeight`.

### Height calculation contract

Card height for virtual-scroll positioning and compact rendering is governed by a **single canonical function** in `+page.svelte`:

```
estimatedCardHeight(item)   →  total occupied height (content + cardGap)
compactCardHeightFor(item)  →  estimatedCardHeight(item) - compactCardGap   (CSS height for compact cards)
virtualHeightFor(item)      →  measuredCardHeights[item.id] ?? estimatedCardHeight(item)
```

Rules:

- **`estimatedCardHeight`** is the single source of truth. It handles all item kinds (text, link, image, file) and both compact/non-compact modes. The returned value always includes `compactCardGap` in compact mode.
- **`compactCardHeightFor`** delegates to `estimatedCardHeight - compactCardGap`. It returns `0` in non-compact mode (card auto-sizes). Do not add independent logic to this function.
- **`virtualHeightFor`** is the virtual-scroll estimator. It checks `measuredCardHeights` (populated by `ClipboardCard`'s `onheightchange` ResizeObserver) first, then falls back to `estimatedCardHeight`.
- Any future height-affecting change (new item kind, new layout option, compact metric) must be implemented in `estimatedCardHeight` only. The other functions will stay consistent automatically.
- `itemHeight()` in `virtual-scroll.ts` is a shared helper used internally by `estimatedCardHeight` for text/image formula computation. It must match the compact-card formula so that `estimatedCardHeight - compactCardGap` equals the CSS height set on `ClipboardCard`.

### `DetailPanel.svelte`

Owns overlay/split detail rendering, resource metadata display, OCR status/actions, detected-content actions, code/Markdown preview, editing, rename/duplicate/save-as-new, file actions, and copy/plain-paste/format-paste/clean-paste callbacks.

Image fullscreen is delegated to `ImageFullscreenOverlay.svelte` via the `onimagefullscreen` callback; DetailPanel no longer owns the fullscreen viewer or WebviewWindow lifecycle.

Key contracts:

- `item` may be null and `mode` is `overlay` or `split`.
- Async OCR listeners must be unregistered when the item changes, the panel closes, or the component is destroyed.
- Keep resource metadata parsing consistent with `clipboard.ts` and backend `resource_metadata.rs`.
- The Details tab renders an editable tag row: chips with remove `×`, plus an inline input/`+` that calls `onsavetags(id, tags)` (full replacement). It reads/writes `item.tags`.

### `ImageFullscreenOverlay.svelte`

Standalone fullscreen image viewer overlay for in-app overlay mode. Renders at `z-index: 200` with zoom, pan, drag, double-click reset, and Escape/close-button dismissal. Uses `window.addEventListener("keydown", ..., true)` in capture phase with `stopPropagation` to intercept Escape before other handlers.

Key contracts:

- Props: `filePath: string`, `opacity: number` (0–1), `onclose: () => void`.
- Owns all viewer state (zoom, pan, drag) and destroys listener on unmount.
- Close paths: Escape key (capture phase), X button (`onclick`), no backdrop-click-to-close.

### Desktop fullscreen viewer (`+page.svelte`)

When `imageFullscreenMode` is `"desktop"`, `handleImageFullscreen` calls `openDesktopViewer()` which creates a raw DOM container, enters the Fullscreen API (`requestFullscreen`), and supports the same zoom/pan/drag/double-click-reset behavior. The zoom hint is displayed as a fixed overlay. Close via Escape key, close button, or `fullscreenchange` event. All event listeners are cleaned up on close.

### `ContextMenu.svelte`

Renders viewport-bounded menu items with `id`, label, icon, destructive state, and disabled state. It closes on Escape or outside click and dispatches `onaction(id)`. It currently does not implement arrow-key item navigation; do not document or rely on that behavior without implementing and testing it.

## Content preview/editing

| Component                | Responsibility                                                               | Important boundary                                                                             |
| ------------------------ | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `CodePreview.svelte`     | Local regex/token-set language detection and syntax coloring                 | No external highlighting package is used; content must remain escaped through Svelte rendering |
| `CodeEditor.svelte`      | Controlled `contenteditable` editor with line numbers and live `CodePreview` | Normalizes newlines, handles Tab/Enter/paste, reports all edits through `oncontentchange`      |
| `MarkdownPreview.svelte` | Small local Markdown-to-HTML renderer                                        | Sanitize generated HTML and restrict URL schemes; do not introduce raw untrusted HTML paths    |

These preview surfaces have niche palettes recorded in `niche_ui_style.md`; do not use them as project-wide theme examples.

## Settings composition

### `StorageSettingsDialog.svelte`

This is the settings shell and an integration hotspot. It owns:

- modal versus standalone sizing;
- primary navigation, global settings search, result targeting, breadcrumb, secondary row, description, count, and optional close button;
- composition of child settings panels with `showHeader={false}`;
- built-in storage, OCR, statistics/performance/memory, icon management, database/search tools, data import/export, and restart-required flows;
- the built-in About section: app version and GitHub Releases update check via `checkForUpdate()`/`update.ts`, with up-to-date/available/error states and release notes;
- the `--settings-*` semantic metrics consumed by child panels.

Do not treat its long scoped style block as a copy template. Use `settings-shared.css` and `settings-panels.md` for approved reusable primitives.

### Child settings panels

| Component                         | Focus                                                  | Extra routing props                                                |
| --------------------------------- | ------------------------------------------------------ | ------------------------------------------------------------------ |
| `GeneralSettingsPanel.svelte`     | Window, search, item, and display preferences          | `section`: `"search"`, `"items"`, `"display"`, or `"window"`       |
| `ThemeSettingsPanel.svelte`       | Dark/light/custom modes and named preset CRUD          | none beyond shell props                                            |
| `FontSizeSettingsPanel.svelte`    | Interface and card font controls                       | internal `interface`/`card` subview; emits `settings-font-changed` |
| `CompactSettingsPanel.svelte`     | Compact toggle and dimensions                          | relies entirely on shared CSS                                      |
| `KeyboardSettingsPanel.svelte`    | Multiple shortcuts per action and recording            | `category`: `"item"`, `"quick"`, or `"system"`; `resetToken`       |
| `IgnoredAppsSettingsPanel.svelte` | Capture pause, discovered apps, ignore list, app icons | optional `iconsDir`                                                |

Every child panel accepts `onclose` and optional `showHeader`. The parent must render it with `showHeader={false}` so headers are removed from the DOM rather than hidden across Svelte style scopes.

## Shared utility components

| Component             | Contract                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AppIcon.svelte`      | Typed `IconName` union, SVG uses `currentColor`, optional size/stroke/fill. Extend the union and rendering branch together.                                                                                                                                                                                                                                                                                                                                                                                                       |
| `Toast.svelte`        | Global toast subscriber, respects `showToastNotifications`, animates leaving entries. Producers call `showToast` in `services/toast.ts`.                                                                                                                                                                                                                                                                                                                                                                                          |
| `DatePicker.svelte`   | Locale-aware calendar popover used by the storage export date range. Props: `value: string` (`YYYY-MM-DD` or `""`), `onchange(value)`, `ariaLabel`, `disabled`. Month/weekday names and display format come from the app `locale` via `Intl`; weeks start Monday for `zh-CN`, Sunday for `en`. Replaces native `<input type="date">` (whose picker language follows the webview/OS, not the app locale). Opens into a `position: fixed` popover with the shared `.popover-surface` visual; closes on outside click/scroll/Escape. |
| `CustomSelect.svelte` | All settings dropdowns use this shared component; see its `Props` interface for `value`/`options`/`onchange`/`className`. Native `<select>`/`<option>` is not allowed in settings.                                                                                                                                                                                                                                                                                                                                                |

## Component change checklist

- Update all call sites when props/callbacks change.
- Preserve focus, Escape, and outside-click ordering across route, detail, context menu, dialogs, and windows.
- Clean up store subscriptions, Tauri event listeners, observers, timers, and WebviewWindow references.
- Update i18n dictionaries and types together for user-visible strings.
- For visual changes, read `css-theming.md`; for settings, also read `settings-panels.md`.
- Update this reference when component ownership or a durable interaction contract changes.
