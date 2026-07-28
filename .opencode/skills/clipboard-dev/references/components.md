# Frontend Components

Read each component's current `Props` interface before changing a call site. This reference records ownership and invariants, not an exhaustive signature snapshot.

## Main workflow components

### `ClipboardCard.svelte`

Owns one list item: text/link/image/file rendering, source metadata, quick/content actions, context-menu actions, inline editing, favorite/delete/restore/copy/detail/plain-paste callbacks, compact layout, and measurement reporting.

Key contracts:

- The route owns collection state and persistence decisions; the card emits controlled callbacks keyed by item ID.
- `onheightchange` plus `heightMeasurementKey` feed virtual scrolling. Re-measure whenever visible content, compact metrics, action/meta visibility, title state, or line limits can alter height.
- Resource previews use `convertFileSrc`; source app icons are resolved from the managed `iconsDir` store using the icon key, not an arbitrary full path.
- Card action order and context-menu behavior must stay aligned. Reuse the same callback path rather than creating a second implementation.
- Compact dimensions come from `GeneralSettings` and must remain aligned with `virtual-scroll.ts` and route height calculations.

### `DetailPanel.svelte`

Owns overlay/split detail rendering, image fullscreen entry, resource metadata display, OCR status/actions, detected-content actions, code/Markdown preview, editing, rename/duplicate/save-as-new, file actions, and copy/plain-paste callbacks.

Key contracts:

- `item` may be null and `mode` is `overlay` or `split`.
- All fullscreen close paths must run the complete close routine so WebviewWindow/fullscreen state, listeners, and component state are released together.
- Async OCR and viewer listeners must be unregistered when the item changes, the panel closes, or the component is destroyed. If listener registration resolves after its scope was invalidated, call the returned unlisten function immediately.
- Keep resource metadata parsing consistent with `clipboard.ts` and backend `resource_metadata.rs`.

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
- built-in storage, OCR, statistics/performance/memory, icon management, database/search tools, and restart-required flows;
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

| Component        | Contract                                                                                                                                 |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `AppIcon.svelte` | Typed `IconName` union, SVG uses `currentColor`, optional size/stroke/fill. Extend the union and rendering branch together.              |
| `Toast.svelte`   | Global toast subscriber, respects `showToastNotifications`, animates leaving entries. Producers call `showToast` in `services/toast.ts`. |

## Component change checklist

- Update all call sites when props/callbacks change.
- Preserve focus, Escape, and outside-click ordering across route, detail, context menu, dialogs, and windows.
- Clean up store subscriptions, Tauri event listeners, observers, timers, and WebviewWindow references.
- Update i18n dictionaries and types together for user-visible strings.
- For visual changes, read `css-theming.md`; for settings, also read `settings-panels.md`.
- Update this reference when component ownership or a durable interaction contract changes.
