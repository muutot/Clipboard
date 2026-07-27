# Services — Detailed Reference

## settings.ts

- **Exports**: `DEFAULT_GENERAL_SETTINGS`, `generalSettings` (Svelte writable store), `readSettingsFromBackend()`, `syncSettingsToBackend()`, `persistGeneralSettings()` (debounced 120ms), `loadAndApplySettings()`, `setWindowConfig()`, `getWindowConfig()`, `setWindowPosition()`
- **Pattern**: Svelte writable store for reactive state, debounced persistence (120ms), bidirectional sync between frontend and backend

## clipboard.ts

- **Exports**: `writeClipboardText(text)`, `writeClipboardImage(blob, resourcePath?, contentHash?)`, `loadClipboardHistory(limit, offset)`, `loadRecycleBinHistory(limit, offset)`, `searchClipboardHistory(query, limit, offset, sortRules?)`, `toClipboardItem(record)`, `detectContentActions(item)`, `getDisplayTitle(item)`, `getDisplayRemainingLines(item)`
- **Pattern**: `writeClipboardText` calls `mark_self_triggered` before write to prevent self-copy loop
- **Search pagination**: `searchClipboardHistory` accepts `offset` for pagination; first call offset=0, subsequent calls increment by pageSize

## toast.ts

- **Exports**: `ToastType` (type), `ToastEntry` (interface), `onToast(callback): unsub`, `showShowToast(message, type?, duration?)`
- **Pattern**: Listener-based pub/sub, unique ID generation, auto-remove after duration

## storage.ts

- **Exports**: `StorageStatus`, `StorageDirectoryUpdate`, `StorageConfig`, `ResourceStorageUpdate`, `PerformanceMetrics`, `RepairResult`, `SearchSyncSummary`, `StorageKind`, `StorageKindStats`, `StorageKindDeleteResult`, `IconFileInfo`
- **Functions**: `configureStorageDirectory()`, `getStorageKindStats()`, `getStorageConfig()`, `getStorageStatus()`, `permanentlyDeleteStorageKind()`, `rebuildSearchIndex()`, `getPerformanceMetrics()`, `repairDatabase()`, `setResourceStoragePaths()`, `validateSearchIndex()`, `listIconFiles()`, `deleteIconFiles()`
- **Pattern**: All functions use `invoke()` to call Tauri commands

## keyboard.ts

- **Exports**: `KeyboardConfig` (interface with `shortcuts: Record<string, string[]>`), `getKeyboardConfig()`, `configureKeyboardShortcuts(action, shortcuts)`, `deleteKeyboardAction(action)`, `resetKeyboardConfig()`
- **Pattern**: CRUD operations for keyboard shortcuts via Tauri invoke

## capture.ts

- **Exports**: `DiscoveredApplication` (type), `ApplicationFilterSettings` (type), `getApplicationFilterSettings()`, `configureIgnoredApplications(applications)`
- **Pattern**: Application discovery and filtering for clipboard capture

## runtime.ts

- **Exports**: `isTauriRuntime()`, `getRuntimeInfo()`
- **Pattern**: `isTauriRuntime()` checks `window.__TAURI_INTERNALS__` existence

## paths.ts

- **Exports**: `iconsDir` (Svelte writable store)
- **Pattern**: Icons directory path management

## memory.ts

- **Exports**: `getMemoryDiagnostics()`
- **Returns**: `MemoryDiagnostics | null` (nullable)

## settings-bootstrap.ts

- **Exports**: `applyGeneralSettingsToDocument(settings)`, `syncCompactShellClass(compactMode)`
- **Pattern**: Sets CSS custom properties on `document.documentElement` (21 color vars + 5 font-size vars), toggles `.compact` class on `.app-shell`
- **Called during**: App startup initialization
