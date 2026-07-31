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

| Primary category | Secondary sections / implementation                                                                     |
| ---------------- | ------------------------------------------------------------------------------------------------------- |
| General          | Window, Search, Items, Display → `GeneralSettingsPanel`                                                 |
| Appearance       | Theme, Font, Compact → `ThemeSettingsPanel`, `FontSizeSettingsPanel`, `CompactSettingsPanel`            |
| Capture          | Single current section → `IgnoredAppsSettingsPanel`                                                     |
| Storage          | Paths, Limits, Tools (icon cache, search index, database repair, import/export) → built into the parent |
| Keyboard         | Item, Quick, System → `KeyboardSettingsPanel` with category prop                                        |
| OCR              | Single current section → built into the parent                                                          |
| Statistics       | Storage, Performance, Memory → built into the parent                                                    |

The left sidebar retains primary categories. The right content pane owns global settings search, item counts, breadcrumb, secondary row, description, and the selected panel.

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

`src/lib/settings-search.ts` is the navigation/search metadata for settings. When adding, renaming, or moving a setting card:

1. update the panel and translations;
2. update settings-search metadata/section routing;
3. ensure the target card exposes searchable text through its heading/description/label;
4. verify result navigation selects the correct section and highlights the intended element;
5. update item count/search result behavior if the card uses a nonstandard container.

## Shared versus panel-specific CSS

Shared base classes belong in `settings-shared.css`. Panel-specific examples include theme color inputs/presets, keyboard binding chips, font numeric inputs, general sort-rule controls, and the ignored-app transfer board.

Before adding CSS:

1. search `settings-shared.css`, the parent shell, and sibling panels for the same primitive;
2. use existing `--settings-*` and theme variables;
3. avoid redefining header/card/toggle/slider/feedback rules in a child;
4. if an existing child duplicates them, do not copy the duplication—record or address it as a narrow refactor;
5. test standalone and parent-composed rendering when `showHeader` or sizing is affected.

## Form details

- Hide number spin buttons when the control is visually a plain value field.
- Use `appearance: none` for selects only with a deliberate replacement affordance.
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
