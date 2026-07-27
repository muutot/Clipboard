# GeneralSettings — Fields and Defaults

All fields in `GeneralSettings` (from `src/lib/types/clipboard.ts`) with default values (from `src/lib/services/settings.ts`).

## Scalar Fields

| Field | Type | Default |
|---|---|---|
| `language` | `"zh-CN" \| "en"` | `"zh-CN"` |
| `windowTransparency` | `number` | `95` |
| `compactMode` | `boolean` | `false` |
| `pinCopiedToTop` | `boolean` | `true` |
| `useRecycleBin` | `boolean` | `true` |
| `showToastNotifications` | `boolean` | `true` |
| `rememberWindowPosition` | `boolean` | `false` |
| `alwaysOnTop` | `boolean` | `false` |
| `useSystemTitleBar` | `boolean` | `false` |
| `theme` | `"dark" \| "light" \| "custom"` | `"dark"` |
| `imageFullscreenMode` | `"overlay" \| "desktop"` | `"overlay"` |
| `viewerBackdropOpacity` | `number` | `92` |
| `searchSuggestionMode` | `"off" \| "panel" \| "inline"` | `"off"` |
| `searchHistoryEnabled` | `boolean` | `false` |
| `cardActionsDisplay` | `"hover" \| "always"` | `"hover"` |
| `quickCopyBadgeAlwaysVisible` | `boolean` | `true` |
| `showSettingsCloseButton` | `boolean` | `true` |
| `detailDisplayMode` | `"overlay" \| "split"` | `"overlay"` |
| `pageSizeLimit` | `number` | `500` |
| `searchPageSizeLimit` | `number` | `500` |
| `searchCacheSize` | `number` | `500` |
| `searchCacheEviction` | `"fifo" \| "lru"` | `"fifo"` |
| `loadTolerance` | `number` | `100` |

## Nested Objects

| Field | Type | Default |
|---|---|---|
| `fontSizes` | `{ base: 14, secondary: 11, tiny: 10, cardTitle: 13, cardPreview: 11 }` | See values |
| `display` | `{ showSecondaryText: true, maxTextLines: 3, pageSize: 100 }` | See values |
| `themeColors` | `ThemeColors` | `{ ...DARK_THEME_COLORS }` |
| `customPresets` | `ThemePreset[]` | `[]` |
| `activePresetId` | `string \| undefined` | `undefined` |
| `searchSortRules` | `SortRule[]` | `[{ field: "createdAt", direction: "desc" }]` |

## Compact Mode Fields (10 fields)

| Field | Default |
|---|---|
| `compactPaddingTop` | `6` |
| `compactPaddingBottom` | `4` |
| `compactCardGap` | `5` |
| `compactTextHeight` | `58` |
| `compactTallTextHeight` | `70` |
| `compactImageHeight` | `130` |
| `compactCustomTitleHeight` | `80` |
| `compactSearchHeight` | `40` |
| `compactSearchFontSize` | `14` |
| `compactCardBorderRadius` | `10` |

## Additional Types

| Type | Description |
|---|---|
| `ThemeColors` | 20 color properties for full custom theming |
| `LIGHT_THEME_COLORS` | Light theme preset |
| `ThemePreset` | Named custom theme with id + colors |
| `FontSizeSettings` | 5 numeric font sizes |
| `DisplaySettings` | showSecondaryText, maxTextLines, pageSize |
| `SortRule` | field + direction for sort |
| `SortField` | 6 sortable fields |
| `ResourceFileMetadata` | Per-file metadata (name, size, extension, mime, hashes, timestamps) |
| `ResourceMetadata` | Wraps file-level metadata (schema version, dimensions, file list) |
| `WindowPosition` | x, y, width, height |
| `WindowConfig` | launchAtStartup, closeToTray, singleInstance |
| `AppSettings` | general + capture |
| `RuntimeInfo` | appVersion, OS, architecture, capabilities |
| `ClipboardFilter` | `"all" \| ClipboardKind \| "favorite" \| "deleted"` |
