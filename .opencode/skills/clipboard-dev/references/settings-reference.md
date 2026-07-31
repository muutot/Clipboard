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
| `compactMode`              | `false` | boolean          |
| `compactPaddingTop`        | `6`     | 0–20             |
| `compactPaddingBottom`     | `4`     | 0–20             |
| `compactCardGap`           | `5`     | 0–20             |
| `compactTextHeight`        | `58`    | 36–90            |
| `compactTallTextHeight`    | `70`    | 44–100           |
| `compactImageHeight`       | `130`   | 64–200           |
| `compactCustomTitleHeight` | `80`    | 40–120           |
| `compactSearchHeight`      | `40`    | 28–56            |
| `compactSearchFontSize`    | `14`    | 10–24            |
| `compactCardBorderRadius`  | `10`    | 0–20             |

`CompactSettingsPanel` currently exposes nine sliders plus the enable switch; `compactCustomTitleHeight` is normalized and consumed by layout logic but is not present in that panel's slider array. Treat that as current behavior, not as proof that the field can be removed.

## General scalar/enum settings

| Field                         | Type/allowed values                | Default     | Range when numeric |
| ----------------------------- | ---------------------------------- | ----------- | ------------------ |
| `language`                    | `"zh-CN"` or `"en"`                | `"zh-CN"`   | —                  |
| `windowTransparency`          | number                             | `95`        | 60–100             |
| `windowEffect`                | `"off"`, `"acrylic"`, or `"mica"`  | `"off"`     | —                  |
| `pinCopiedToTop`              | boolean                            | `true`      | —                  |
| `useRecycleBin`               | boolean                            | `true`      | —                  |
| `showToastNotifications`      | boolean                            | `true`      | —                  |
| `rememberWindowPosition`      | boolean                            | `false`     | —                  |
| `alwaysOnTop`                 | boolean                            | `false`     | —                  |
| `useSystemTitleBar`           | boolean                            | `false`     | —                  |
| `theme`                       | `"dark"`, `"light"`, or `"custom"` | `"dark"`    | —                  |
| `imageFullscreenMode`         | `"overlay"` or `"desktop"`         | `"overlay"` | —                  |
| `viewerBackdropOpacity`       | number                             | `92`        | 0–100              |
| `searchSuggestionMode`        | `"off"`, `"panel"`, or `"inline"`  | `"off"`     | —                  |
| `searchHistoryEnabled`        | boolean                            | `false`     | —                  |
| `cardActionsDisplay`          | `"hover"` or `"always"`            | `"hover"`   | —                  |
| `quickCopyBadgeAlwaysVisible` | boolean                            | `true`      | —                  |
| `showSettingsCloseButton`     | boolean                            | `true`      | —                  |
| `detailDisplayMode`           | `"overlay"` or `"split"`           | `"overlay"` | —                  |
| `pageSizeLimit`               | number                             | `500`       | 500–6000           |
| `searchPageSizeLimit`         | number                             | `500`       | 50–1000            |
| `searchCacheSize`             | number                             | `500`       | 200–2000           |
| `searchCacheEviction`         | `"fifo"` or `"lru"`                | `"fifo"`    | —                  |
| `loadTolerance`               | number                             | `100`       | 50–500             |

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

When backend startup, validation, or native behavior needs one of these fields, add a typed Rust field with a default and tests instead of parsing it opportunistically from `extra`.

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
