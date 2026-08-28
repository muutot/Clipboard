# General Settings Fields, Defaults, and Ranges

Sources of truth:

- Type shape: `src/lib/types/clipboard.ts::GeneralSettings`
- Frontend defaults and normalization: `src/lib/services/settings.ts`
- Typed backend fields/defaults: `src-tauri/src/config.rs::GeneralConfig`
- Persistence flow: `services/settings.ts` and Tauri `get_general_settings` / `set_general_settings`

Do not update this table from UI labels alone. Verify the type, default, normalizer, Rust config, and actual consumer.

## Contents

- [Nested settings](#nested-settings)
- [Compact-mode settings](#compact-mode-settings)
- [General scalar/enum settings](#general-scalarenum-settings)
- [Theme and sort structures](#theme-and-sort-structures)
- [Related but separate settings](#related-but-separate-settings)
- [Backend representation caveat](#backend-representation-caveat)
- [Settings update checklist](#settings-update-checklist)

## Nested settings

| Field                       | Default | Normalized range/values |
| --------------------------- | ------- | ----------------------- |
| `fontSizes.base`            | `14`    | 11–20                   |
| `fontSizes.secondary`       | `11`    | 9–16                    |
| `fontSizes.tiny`            | `10`    | 8–13                    |
| `fontSizes.cardTitle`       | `13`    | 10–20                   |
| `fontSizes.cardPreview`     | `11`    | 8–16                    |
| `display.showSecondaryText` | `true`  | boolean                 |
| `display.maxTextLines`      | `3`     | 1–12                    |
| `display.pageSize`          | `100`   | 50–500                  |
| `display.searchPageSize`    | `100`   | 50–500                  |

## Compact-mode settings

| Field                      | Default | Normalized range |
| -------------------------- | ------- | ---------------- |
| `compactMode`              | `true`  | boolean          |
| `compactPaddingTop`        | `1`     | 0–20             |
| `compactPaddingBottom`     | `1`     | 0–20             |
| `compactCardGap`           | `1`     | 0–20             |
| `compactTextHeight`        | `44`    | 36–90            |
| `compactTallTextHeight`    | `44`    | 44–100           |
| `compactImageHeight`       | `80`    | 64–200           |
| `compactCustomTitleHeight` | `80`    | 40–120           |
| `compactSearchHeight`      | `30`    | 28–56            |
| `compactSearchFontSize`    | `20`    | 10–24            |
| `compactCardBorderRadius`  | `5`     | 0–20             |

`CompactSettingsPanel` currently exposes nine sliders plus the enable switch; `compactCustomTitleHeight` is normalized and consumed by layout logic but is not present in that panel's slider array. Treat that as current behavior, not as proof that the field can be removed.

## General scalar/enum settings

| Field                         | Type/allowed values                | Default     | Range when numeric                                                               |
| ----------------------------- | ---------------------------------- | ----------- | -------------------------------------------------------------------------------- |
| `language`                    | `"zh-CN"` or `"en"`                | `"zh-CN"`   | —                                                                                |
| `windowTransparency`          | number                             | `95`        | 60–100                                                                           |
| `windowEffect`                | `"off"`, `"acrylic"`, or `"mica"`  | `"off"`     | —                                                                                |
| `windowOpacityAffectsText`    | boolean                            | `false`     | —                                                                                |
| `pinCopiedToTop`              | boolean                            | `true`      | —                                                                                |
| `useRecycleBin`               | boolean                            | `true`      | —                                                                                |
| `pasteCleaningEnabled`        | boolean                            | `false`     | —                                                                                |
| `doubleClickPaste`            | boolean                            | `true`      | —                                                                                |
| `maxTextCaptureBytes`         | number (bytes)                     | `500_000`   | 10000–10000000                                                                   |
| `showToastNotifications`      | boolean                            | `true`      | —                                                                                |
| `rememberWindowPosition`      | boolean                            | `false`     | —                                                                                |
| `alwaysOnTop`                 | boolean                            | `false`     | —                                                                                |
| `useSystemTitleBar`           | boolean                            | `false`     | —                                                                                |
| `theme`                       | `"dark"`, `"light"`, or `"custom"` | `"dark"`    | —                                                                                |
| `imageFullscreenMode`         | `"overlay"` or `"desktop"`         | `"overlay"` | —                                                                                |
| `viewerBackdropOpacity`       | number                             | `92`        | 0–100                                                                            |
| `searchSuggestionMode`        | `"off"`, `"panel"`, or `"inline"`  | `"off"`     | —                                                                                |
| `searchHistoryEnabled`        | boolean                            | `false`     | —                                                                                |
| `searchPlaceholder`           | string                             | `""`        | trimmed, ≤80 chars; empty = use localized default text (`app.searchPlaceholder`) |
| `cardActionsDisplay`          | `"hover"` or `"always"`            | `"hover"`   | —                                                                                |
| `quickCopyBadgeAlwaysVisible` | boolean                            | `true`      | —                                                                                |
| `showSettingsCloseButton`     | boolean                            | `true`      | —                                                                                |
| `detailDisplayMode`           | `"overlay"` or `"split"`           | `"overlay"` | —                                                                                |
| `pageSizeLimit`               | number                             | `500`       | 500–6000                                                                         |
| `searchPageSizeLimit`         | number                             | `500`       | 50–1000                                                                          |
| `searchCacheSize`             | number                             | `500`       | 200–2000                                                                         |
| `searchCacheEviction`         | `"fifo"` or `"lru"`                | `"fifo"`    | —                                                                                |
| `searchIndexSyncMode`         | `"lazy"` or `"background"`         | `"lazy"`    | —                                                                                |
| `updateSource`                | `"github"` or `"gitcode"`          | `"gitcode"` | —                                                                                |
| `colorIcons`                  | boolean                            | `false`     | —                                                                                |
| `iconColors`                  | `IconColors` map (hex strings)     | `{}`        | keys limited to `ICON_NAMES`; values must be `#rrggbb`/`#rrggbbaa`               |
| `loadTolerance`               | number                             | `100`       | 50–500                                                                           |

## Theme and sort structures

| Field             | Default                                       | Contract                                                                                                |
| ----------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `themeColors`     | copy of `DARK_THEME_COLORS`                   | 20 validated hex colors; optional in the interface for compatibility but always filled by normalization |
| `customPresets`   | `[]`                                          | array of named `ThemePreset` objects with valid colors                                                  |
| `activePresetId`  | `undefined`                                   | string only when a named preset is active                                                               |
| `searchSortRules` | `[{ field: "createdAt", direction: "desc" }]` | fields: createdAt, lastUsedAt, title, size, kind, favorite; direction asc/desc                          |

See `css-theming.md` for the full ThemeColors → CSS variable contract.

## Related but separate settings

`WindowConfig` is retrieved/saved by separate commands and currently defaults to launch-at-startup false, close-to-tray true, and single-instance true. Position is stored in the same backend window config group.

History/storage/OCR/privacy/export settings are separate Rust config groups and settings commands. Keyboard bindings remain in `conf/keyboard.json`. Do not add their fields to `GeneralSettings` merely because the controls appear in the same settings window.

## Backend representation caveat

`GeneralConfig` explicitly types a core subset of the frontend settings and flattens unknown keys. Fields such as theme colors/presets, detail/card display options, search sort/cache policy, and load tolerance can survive through the flattened map without being explicit Rust members.

`search_index_sync_mode` is an explicit `GeneralConfig` member (`"lazy"`/`"background"`, default `"lazy"`) because the backend startup wiring and the search command read it for typed behavior. `ConfigStore::search_index_sync_mode()` returns `SearchIndexSyncMode`. Changing the mode only takes effect after restart: the `SearchSyncWorker` is created at startup when the mode is `background`.

`update_source` is an explicit `GeneralConfig` member (`"github"`/`"gitcode"`, default `"gitcode"`) with a typed `UpdateSource` in `config/types.rs`. `ConfigStore::update_source()` returns `UpdateSource`, and the About-panel `check_for_update` reads it per call, so switching the dropdown applies immediately without a restart.

`max_text_capture_bytes` is an explicit `GeneralConfig` member (`u64` bytes, default `500_000`) because the capture loop reads it per iteration for typed behavior. `ConfigStore::max_text_capture_bytes()` clamps to 10000–10000000. At startup `CaptureState::new` receives the value into an `Arc<AtomicU64>`; `set_general_settings` also pushes the saved value into the capture state, so a slider change applies immediately without a restart. The capture loop caps plain text, HTML, and RTF captures at this limit; over-limit content is skipped.

When backend startup, validation, or native behavior needs one of these fields, add a typed Rust field with a default and tests instead of parsing it opportunistically from `extra`.

## Sync settings

These defaults come from `SyncConfig::default` and are also the values serialized for a fresh `conf.json`; `get_sync_config` returns the complete effective policy so the settings UI does not have to reconstruct missing backend fields.

| Field                       | Type             | Default           | Range    | Description                                                                                  |
| --------------------------- | ---------------- | ----------------- | -------- | -------------------------------------------------------------------------------------------- |
| `sync.autoSync`             | `bool`           | `false`           | boolean  | Enable auto-sync on clipboard change                                                         |
| `sync.autoSyncIntervalSecs` | `u64`            | `300`             | 10–86400 | Auto-sync interval in seconds                                                                |
| `sync.segmentMaxEntries`    | `u32`            | `512`             | 16–10000 | Maximum coalesced mutations in one immutable v1 segment                                      |
| `sync.maxSyncImageBytes`    | `u64`            | `5242880` (5MB)   | 0–       | Max image size to sync (0=disabled)                                                          |
| `sync.maxSyncFileBytes`     | `u64`            | `10485760` (10MB) | 0–       | Max file size to sync (0=disabled)                                                           |
| `sync.remotePath`           | `Option<String>` | `clipboard-sync`  | —        | Object-key prefix containing the isolated v1 namespace                                       |
| `sync.s3Region`             | `Option<String>` | `us-east-1`       | —        | S3 signing region                                                                            |
| `sync.s3Bucket`             | `Option<String>` | `None`            | —        | S3 bucket name                                                                               |
| `sync.s3AccessKey`          | `Option<String>` | `None`            | —        | S3 access key                                                                                |
| `sync.s3SecretKey`          | `Option<String>` | `None`            | —        | S3 secret key (write-only; `hasS3SecretKey` flag returned)                                   |
| `sync.syncPassword`         | `Option<String>` | `None`            | —        | Optional password encrypting v1 metadata packs (write-only; `hasSyncPassword` flag returned) |
| `sync.lastSyncMs`           | `Option<i64>`    | `None`            | —        | Last attempted sync timestamp (auto-managed)                                                 |
| `sync.lastSyncStatus`       | `Option<String>` | `None`            | —        | Last sync status (auto-managed)                                                              |

`SyncProvider` has only `off` and `s3`. The settings UI exposes no WebDAV, remote-backup list,
download, verification, compaction, baseline, or oplog controls. `segmentMaxEntries` bounds local
memory per incremental pack; it is not a mutable-log rollover policy.

## Settings update checklist

1. Change the TypeScript interface and related unions.
2. Change `DEFAULT_GENERAL_SETTINGS`.
3. Change normalizer validation/range and nested clone behavior.
4. Change `GeneralConfig` and backend behavior when the backend needs typed access.
5. Add/update UI and settings-search metadata.
6. Apply the value live or mark restart-required.
7. Update English, Chinese, and typed i18n shape.
8. Test defaults, old-config normalization, persistence, cross-window events, and the consumer.
9. Update this reference, `data-contracts.md`, and style references if applicable.
