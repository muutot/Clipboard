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

- writing text/image/html with self-trigger registration;
- loading active and deleted pages (`loadClipboardHistory(limit, offset, filter?)` passes an optional `HistoryFilterArgs` `{kind, favorite, tag, sourceApp, dateFromMs, dateToMs}` to `list_clipboard_items` so filtered pages are fetched from the backend);
- searching with offset, limit, and optional sort rules;
- favorite/delete/restore/permanent/batch operations;
- tag persistence (`persistTags(id, tags)` invokes `set_clipboard_item_tags`);
- tag management wrappers for the settings tag manager: `listAllTags()` (`list_all_tags` → `TagInfo[] {name,count,color}`), `renameTag(old,new)` (`rename_tag` → affected-count), `deleteTag(name)` (`delete_tag`), and `setTagColor(name,color)` (`set_tag_color`);
- source-app listing and content-action detection;
- `PersistedClipboardItem` → `ClipboardItem` mapping (including `tags` parsed from `metadata_json.tags`);
- resource metadata parsing and display title/size helpers.

Keep frontend mapping aligned with Rust serde names and `metadata_json`. `loadDeletedClipboardHistory` is the current deleted-list API name. A copy/write path must register a compatible self-trigger hash before touching the system clipboard.

## Invoke-wrapper services

| Service       | Responsibility                                                                                                                                                                                                                                                                                                                                | Runtime fallback                     |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| `storage.ts`  | storage status/config, per-kind stats/deletion, path changes, search rebuild/validation, DB repair, performance, icon cache (list/delete/replace), export/import (`get_export_formats`, `get_import_formats` with `{id,label,extension}` entries, `export_to_file` with favorites/date-range/content-type filter options, `import_from_file`) | nullable/empty values where declared |
| `keyboard.ts` | get/configure/delete/reset arrays of shortcuts by action                                                                                                                                                                                                                                                                                      | nullable or command result           |
| `capture.ts`  | discovered apps, current ignore config, update ignored applications                                                                                                                                                                                                                                                                           | nullable or command result           |
| `memory.ts`   | process-group/system/OCR memory diagnostics                                                                                                                                                                                                                                                                                                   | `null` outside Tauri                 |
| `runtime.ts`  | safe `isTauriRuntime` detection and runtime capability query                                                                                                                                                                                                                                                                                  | static browser fallback/`null`       |
| `update.ts`   | `check_for_update` (GitCode latest-version check)                                                                                                                                                                                                                                                                                             | throws outside Tauri                 |

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

Paste paths (`plainPaste`, `formatPaste`, `cleanPaste`) live in the route. When `pasteCleaningEnabled` is set, plain/format paste run `transform_text` with `cleanPaste` first; format paste falls back to writing the cleaned plain text (instead of HTML) whenever cleaning changed the content, since URL/whitespace cleanup cannot be applied reliably inside markup. `formatPaste` passes the stored `rtfContent` to `writeClipboardHtml(html, plainText, rtf)`, which writes `text/rtf` alongside `text/html`/`text/plain` so RTF-preferring apps keep formatted paste.

Double-click paste (`doubleClickPasteItem`, enabled by the `doubleClickPaste` setting which defaults on) also lives in the route: text/link items reuse `formatPaste` when the item has `htmlContent`, otherwise `plainPaste`; image items re-fetch the stored image via `convertFileSrc` and call `writeClipboardImage`; file items re-use the copy-item path (multi-file `textContent` JSON list joined by newlines, or the primary `resourcePath`). All four cases end in `pasteToPreviousApplication`. When `doubleClickPaste` is off, the card's double click falls back to `ondetail`.

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
