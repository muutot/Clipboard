# Components — Detailed Reference

## AppIcon.svelte

- **Props**: `{ name: IconName, size: number = 18, strokeWidth: number = 1.8, filled: boolean = false }`
- **Type**: `IconName` is a union of 39 icon names (e.g., 'arrow-up', 'check', 'copy', 'gear', 'trash')
- **Pattern**: `{#if}/{:else if}` chains to render SVG paths based on `name`

## ClipboardCard.svelte

- **Props**: `{ item: ClipboardItem, onclose?, ondelete?, onfavorite?, ondetail?, oncopy?, onheightchange? }`
- **State**: `assetUrlCache` (Map, MAX_CACHE_SIZE=500), `contextMenuOpen`, `isDetailPanelOpen`
- **Key Features**: Context menu integration, relative time display, lazy image loading, keyboard navigation (Enter=copy, Delete=delete, ArrowUp/Down=navigate)
- **Height reporting**: Uses ResizeObserver with `recordCardHeight(item.id, height)` for virtual scroll. Initial measurement is synchronous via `element.clientHeight`.

## CodePreview.svelte

- **Props**: `{ content: string, language?: string }`
- **State**: `$derived` for `languageLabel` (auto-detection from 10+ patterns) and `highlightedHtml`
- **Pattern**: Custom tokenizer with regex-based tokenization (comments, strings, keywords, numbers, operators)

## CodeEditor.svelte

- **Props**: `{ content: string, language?: string, editorLabel: string, previewLabel: string, placeholder?: string, oncontentchange?: (content: string) => void }`
- **State**: Uses `contenteditable` div, `$derived` for `lineCount` and `languageLabel`, `$effect` for syncing external content changes
- **Key Features**: Tab key handling (inserts tab or spaces), line numbers, live preview via CodePreview

## DetailPanel.svelte

- **Props**: `{ item: ClipboardItem, onclose, oncopy }`
- **State**: `isEditing` toggle, `editedContent` for markdown/code editing, `resourceMetadata` for file info
- **Key Features**: Markdown preview, code preview, resource metadata display, image/file preview

## ContextMenu.svelte

- **Props**: `{ x: number, y: number, items: ContextMenuItem[], onclose: () => void, onaction: (id: string) => void }`
- **Type**: `ContextMenuItem` — `{ id, label, icon?, disabled?, danger?, separator? }`
- **Key Features**: Viewport-boundary-aware positioning, keyboard navigation (ArrowUp/Down, Enter), click outside to close
- **z-index**: 9999

## Toast.svelte

- **Props**: None (global singleton)
- **State**: Subscribes to `generalSettings.showToastNotifications`, `toastStore`, `leaving` state for 2000ms animation
- **Key Features**: Auto-dismiss after duration, leaving animation

## MarkdownPreview.svelte

- **Props**: `{ content: string }`
- **State**: `$derived` for `sanitizedHtml` (sanitizes URLs to http/https/mailto/tel only)

## StorageSettingsDialog.svelte

- **Props**: None (main settings dialog, owns the shell hierarchy)
- **State**: `currentPanel` for navigation, `storageStats`, `searchQuery` for settings search
- **Shell hierarchy**: Breadcrumb → secondary-group tabs → description → setting cards
- **Child panels rendered with `showHeader={false}`**

## Settings Panels (showHeader pattern)

All settings panels accept `{ onclose: () => void, showHeader: boolean = true }`:
- `GeneralSettingsPanel.svelte` — section prop: "search" | "items" | "display" | "window"
- `FontSizeSettingsPanel.svelte` — subnav: "interface" | "card"
- `ThemeSettingsPanel.svelte` — theme mode switch, color pickers, preset CRUD
- `CompactSettingsPanel.svelte` — compact mode toggle + dimension sliders
- `KeyboardSettingsPanel.svelte` — category prop: "item" | "clipboard" | "system"
- `IgnoredAppsSettingsPanel.svelte` — two-column transfer list, privacy pause toggle
