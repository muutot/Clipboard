# Frontend Services and State

Frontend services are the preferred Tauri boundary. Keep direct `invoke` calls in routes/components only for behavior that has not yet earned a reusable wrapper; when several call sites appear, move the contract into a service.

## `settings.ts`

`generalSettings` is a Svelte writable store with `updateSetting`, `merge`, `initialize`, `flush`, and `destroy` helpers.

Desktop flow:

1. start from normalized defaults;
2. subscribe to `general-settings-changed` before hydration;
3. read `get_general_settings` from Rust;
4. migrate legacy browser/localStorage values once when the backend reports they are needed;
5. merge edits made before hydration through dirty-key/revision tracking;
6. debounce writes for 120 ms through `set_general_settings`;
7. apply the command response as canonical and listen for changes from other windows.

Browser-preview flow uses localStorage and the browser `storage` event. Do not describe localStorage as the desktop source of truth.

When changing settings, update normalization, valid unions, numeric ranges, cloning of nested values, legacy migration, backend `GeneralConfig`, UI, and `settings-reference.md` as applicable. Call `flush()` before a lifecycle boundary when pending persistence must be guaranteed.

## `clipboard.ts`

Owns the record boundary and list operations:

- writing text/image with self-trigger registration;
- loading active and deleted pages;
- searching with offset, limit, and optional sort rules;
- favorite/delete/restore/permanent/batch operations;
- source-app listing and content-action detection;
- `PersistedClipboardItem` → `ClipboardItem` mapping;
- resource metadata parsing and display title/size helpers.

Keep frontend mapping aligned with Rust serde names and `metadata_json`. `loadDeletedClipboardHistory` is the current deleted-list API name. A copy/write path must register a compatible self-trigger hash before touching the system clipboard.

## Invoke-wrapper services

| Service       | Responsibility                                                                                                                                                                                                                                                | Runtime fallback                     |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| `storage.ts`  | storage status/config, per-kind stats/deletion, path changes, search rebuild/validation, DB repair, performance, icon files, export/import (`get_export_formats`, `export_to_file` with favorites/date-range/content-type filter options, `import_from_file`) | nullable/empty values where declared |
| `keyboard.ts` | get/configure/delete/reset arrays of shortcuts by action                                                                                                                                                                                                      | nullable or command result           |
| `capture.ts`  | discovered apps, current ignore config, update ignored applications                                                                                                                                                                                           | nullable or command result           |
| `memory.ts`   | process-group/system/OCR memory diagnostics                                                                                                                                                                                                                   | `null` outside Tauri                 |
| `runtime.ts`  | safe `isTauriRuntime` detection and runtime capability query                                                                                                                                                                                                  | static browser fallback/`null`       |
| `update.ts`   | `check_for_update` (GitHub Releases latest-version check)                                                                                                                                                                                                     | throws outside Tauri                 |

Do not silently return `null` from a new wrapper unless the caller can distinguish “not in Tauri” from a command failure. Preserve the existing wrapper's error semantics when extending it.

## UI-only services

| Service                 | Responsibility                                                                                    |
| ----------------------- | ------------------------------------------------------------------------------------------------- |
| `toast.ts`              | listener-based toast pub/sub; public producer is `showToast(message, type?, duration?)`           |
| `paths.ts`              | writable `iconsDir` used to resolve source icon keys                                              |
| `settings-bootstrap.ts` | applies font/theme variables to `document.documentElement` and toggles `.compact` on `.app-shell` |

`settings-bootstrap.ts` is used at startup; per-panel live preview code must remain consistent with it. Do not create a second divergent theme/font mapping.

## Main route state boundaries

`src/routes/+page.svelte` intentionally owns high-level collection/UI state: active/deleted pagination, search request IDs, loaded items, the frontend search-result cache, selection, detail mode, filters, and runtime event reconciliation. Services own IPC and record mapping; cards own rendering and controlled callbacks.

Events currently crossing windows/runtime include:

- `clipboard-item-added`
- `clipboard-history-invalidated`
- `general-settings-changed`
- `settings-font-changed`
- `tray-open-settings`
- `viewer:open`
- OCR download progress events emitted by the backend.

Search exact event names and payload types before modifying producers or consumers; update `data-contracts.md` when a durable event contract changes.

## Service change checklist

- Match frontend invoke argument names with Rust command parameter names and camelCase serialization.
- Update return types, null/error handling, and every caller together.
- Preserve browser-preview behavior when it is intentional; do not let demo behavior masquerade as desktop completion.
- Clean up event listeners and store subscriptions.
- Update tests and the matching service/data reference in the same commit.
