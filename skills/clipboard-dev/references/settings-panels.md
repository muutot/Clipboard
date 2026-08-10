# Settings Shell and Panel Patterns

Read this file and `css-theming.md` before changing settings markup or CSS.

## Contents

- [Navigation and ownership](#navigation-and-ownership)
- [Approved shell hierarchy](#approved-shell-hierarchy)
- [Child panel contract](#child-panel-contract)
- [Settings state pattern](#settings-state-pattern)
- [Canonical card patterns](#canonical-card-patterns)
- [Feedback and asynchronous state](#feedback-and-asynchronous-state)
- [Settings search](#settings-search)
- [Shared versus panel-specific CSS](#shared-versus-panel-specific-css)
- [Form details](#form-details)
- [New or changed setting checklist](#new-or-changed-setting-checklist)

## Navigation and ownership

`StorageSettingsDialog.svelte` is the parent shell for modal and standalone settings. Its primary categories and current secondary sections are:

| Primary category | Secondary sections / implementation                                                                                                                                     |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| General          | General, Window, Search, Items → `GeneralSettingsPanel`; the General page includes the clipboard recording pause/resume toggle                                          |
| Appearance       | Theme, Font, Compact, Icons → `ThemeSettingsPanel`, `FontSizeSettingsPanel`, `CompactSettingsPanel`, `IconColorsSettingsPanel`                                          |
| Capture          | Filter (ignored apps) → `IgnoredAppsSettingsPanel`; Icon cache → built into the parent                                                                                  |
| Tags             | Single current section → `TagManagementSettingsPanel`                                                                                                                   |
| Storage          | Paths, Limits (retention, item count, recycle bin, max file copy size, text capture size), Tools (search index, database repair, import/export) → built into the parent |
| Sync             | Cloud (provider, credentials, manual sync, backups, snapshot refresh), Advanced (batch/resource limits and reserved oplog target) → built into the parent               |
| Keyboard         | Item, Quick, Global → `KeyboardSettingsPanel` with category prop                                                                                                        |
| OCR              | Single current section → built into the parent                                                                                                                          |
| Statistics       | Storage, Performance, Memory → built into the parent                                                                                                                    |
| About            | Version, executable-path location, configurable update source dropdown, and update check (`check_for_update`) → built into the parent                                   |

The left sidebar retains primary categories. The right content pane owns global settings search, item counts, breadcrumb, secondary row, description, and the selected panel.

`src/lib/settings-navigation.ts::SETTINGS_NAV_GROUP_DEFINITIONS` is the shell navigation source of truth. It defines section/statistics-tab types, category order/icons, default targets, secondary-tab order, translated labels, section title/description keys, and breadcrumb behavior; the sidebar, secondary row, and settings-search paths resolve from that module. Update the descriptor plus settings-search item metadata together when a section is added, renamed, or moved.

## Approved shell hierarchy

Use one parent-owned hierarchy:

```text
breadcrumb + item count + optional close button
secondary row (tabs, or one current-section label)
one small description line
scrolling setting cards / panel-specific board
feedback overlay when needed
```

The breadcrumb and description use `--settings-description-size`; secondary section labels and card headings use `--settings-heading-size`. Do not add a second page title inside a child when the parent owns the header.

The current keyboard configuration bar is a local exception to this order and is listed in `niche_ui_style.md`; do not reproduce that placement in new categories.

## Child panel contract

```svelte
<script lang="ts">
  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();
</script>

{#if showHeader}
  <header>...</header>
{/if}

<div class="settings-scroll">...</div>
```

Render every child from `StorageSettingsDialog` with `showHeader={false}`. Conditional removal must happen in the child; a parent scoped selector cannot reliably hide a descendant component's scoped header.

Keep the default `GeneralSettingsPanel` eager. Non-default child panels use cached dynamic imports in `StorageSettingsDialog` so their component code and scoped CSS load on first visit; retain the shared loading/error states, concrete import types, and settings-search mount polling when adding another lazy panel.

`CompactSettingsPanel.svelte` is the minimal structure reference because it uses shared styles without a local style block. Use other panels only for their genuinely specific controls.

## Settings state pattern

```typescript
let s = $state($generalSettings);

$effect(() => {
  const unsubscribe = generalSettings.subscribe((value) => {
    s = value;
  });
  return unsubscribe;
});

function change<K extends keyof GeneralSettings>(key: K, value: GeneralSettings[K]) {
  generalSettings.updateSetting(key, value);
}
```

Do not mutate nested store state in place and assume persistence will notice. Create a new nested object/array and pass it through `updateSetting` or `merge`.

Use `generalSettings.flush()` before a close/restart boundary when the UI must guarantee that the latest debounced write reached the backend.

## Canonical card patterns

### Toggle/control card

```svelte
<section class="setting-card toggle-card">
  <div class="setting-heading">
    <span class="setting-icon"><AppIcon name="..." size={17} /></span>
    <div>
      <strong>{label}</strong>
      <p>{description}</p>
    </div>
  </div>
  <button class="toggle-switch" class:active={enabled} role="switch" aria-checked={enabled}>
    <span class="toggle-knob"></span>
  </button>
</section>
```

A select, segmented language control, number input, or compact action group may replace the toggle while preserving the same left/right hierarchy.

### Slider card

```svelte
<section class="setting-card">
  <div class="setting-heading">
    <span class="setting-icon"><AppIcon name="..." size={17} /></span>
    <div class="heading-inline">
      <div>
        <strong>{label}</strong>
        <p>{description}</p>
      </div>
      <span class="value-label">{value}px</span>
    </div>
  </div>
  <input
    type="range"
    class="transparency-slider"
    style:--slider-pct={percentage}
    oninput={handler}
  />
</section>
```

Do not wrap the range input merely for styling. Initialize/update `--slider-pct` from value/min/max so the filled track is correct on first render and after external store changes.

## Feedback and asynchronous state

- Use `.settings-feedback`; add `.success` for success and default to error styling otherwise.
- Clear feedback with a timer whose cleanup is retained when the component can unmount or messages can be replaced.
- Disable controls or show a saving/loading state during commands that cannot safely overlap.
- Roll optimistic switches back when the backend save or OS synchronization fails.
- Keep restart-required state explicit for path/config changes that do not apply live.

## Settings search

`src/lib/settings-search.ts` owns searchable setting-card metadata and targets; `src/lib/settings-navigation.ts` owns the target types and navigation paths. When adding, renaming, or moving a setting card:

1. update the panel and translations;
2. update settings-search metadata/section routing;
3. ensure the target card exposes searchable text through its heading/description/label;
4. for cards whose heading text cannot reliably match the search title (generic/renamed labels, cards outside `.settings-scroll`, or panel-level targets), set `data-settings-search-id` on the card to the search item id — `findSettingsElement` in `StorageSettingsDialog.svelte` prefers this id lookup, then falls back to heading/text matching, then to the section header;
5. verify result navigation selects the correct section and highlights the intended element;
6. update item count/search result behavior if the card uses a nonstandard container.

Panels that render cards only after async/conditional data (e.g. keyboard config, statistics metrics, tags) are handled by a retry poll in `openSettingsSearchResult`; keep the target card mounted while loading so it is locatable.

## Shared versus panel-specific CSS

Shared base classes belong in `settings-shared.css`, which is imported by the `/settings` route rather than global `app.css` so settings-only rules do not increase or leak into the main-page stylesheet. Panel-specific examples include theme color inputs/presets, keyboard binding chips, font numeric inputs, general sort-rule controls, and the ignored-app transfer board.

Before adding CSS:

1. search `settings-shared.css`, the parent shell, and sibling panels for the same primitive;
2. use existing `--settings-*` and theme variables;
3. avoid redefining header/card/toggle/slider/feedback rules in a child;
4. if an existing child duplicates them, do not copy the duplication—record or address it as a narrow refactor;
5. test standalone and parent-composed rendering when `showHeader` or sizing is affected.

## Form details

- Hide number spin buttons when the control is visually a plain value field.
- Use the shared `CustomSelect.svelte` component for all dropdowns; do not use native `<select>`/`<option>`. Pass `value`, `options` (`{value,label,disabled?}`), and `onchange`; pass a `className` for per-use layout, and adapt the trigger button when a layout override previously targeted `.settings-select` (use `:global()` + `.settings-select` descendant in a component scope).
- Use `DatePicker.svelte` for date fields; do not use native `<input type="date">` (its picker language follows the webview/OS rather than the app locale).
- Keep tabular numeric labels stable and non-shrinking.
- Let long labels/paths/numbers shrink, wrap, or ellipsize without overflowing the card.
- Use semantic status colors through variables and `color-mix`.

## New or changed setting checklist

1. Update `GeneralSettings`, defaults, normalizer/range, nested cloning, and browser migration behavior.
2. Update Rust `GeneralConfig` explicitly when backend behavior/defaults should be typed; do not rely on flattened extras accidentally.
3. Add/update panel UI using the approved shell and shared primitives.
4. Apply live document/window/worker behavior or clearly mark restart-required behavior.
5. Update English, Chinese, and typed i18n shape.
6. Update settings-search metadata.
7. Add tests for normalization/persistence and backend behavior.
8. Update `settings-reference.md`, `data-contracts.md`, and style references when applicable.
9. Run static/build checks plus rendered dark/light/custom and narrow-window verification when visual.
