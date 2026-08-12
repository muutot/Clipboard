# Project-wide UI Style and Theming

This is the authoritative style reference for reusable project UI. Read it before any markup or CSS change. Local exceptions listed in `niche_ui_style.md` are not approved patterns and must not be copied into new work.

## Contents

- [Style source-of-truth order](#style-source-of-truth-order)
- [Approved main-page visual language](#approved-main-page-visual-language)
- [Theme color contract](#theme-color-contract)
- [Global font and display variables](#global-font-and-display-variables)
- [Settings semantic metrics](#settings-semantic-metrics)
- [Shared settings primitives](#shared-settings-primitives)
- [CSS ownership decision](#css-ownership-decision)
- [Form and control conventions](#form-and-control-conventions)
- [Fixed layer order](#fixed-layer-order)
- [Style change gate](#style-change-gate)

## Style source-of-truth order

1. The approved main page in `src/routes/+page.svelte` and `ClipboardCard.svelte` defines the product's visual language.
2. `src/lib/types/clipboard.ts::{DARK_THEME_COLORS,LIGHT_THEME_COLORS}` defines preset values.
3. `src/lib/utils/theme.ts::applyThemeColors` defines the ThemeColors 闂?CSS-variable mapping.
4. `src/app.css` provides root defaults, global reset, font variables, focus/accessibility rules, and imports shared settings CSS.
5. `src/lib/styles/settings-shared.css` owns reusable child-panel primitives.
6. `StorageSettingsDialog.svelte` owns settings-shell layout and built-in storage/OCR/statistics-specific styles.
7. Component-scoped CSS owns only component-specific layout/visual behavior.

When these disagree, inspect the rendered target and current code, fix the narrow source of divergence, and update this reference if the approved rule changes.

## Approved main-page visual language

Preserve these characteristics unless the task explicitly requests a redesign:

- Dense desktop utility layout with one continuous neutral surface, subtle borders, compact spacing, and restrained radii.
- Near-black/dark-neutral surfaces with high-legibility primary text and progressively quieter secondary, muted, and faint text.
- Red `--accent` for product/focus emphasis, blue `--selection-color` for selection/current state, and semantic success/danger/warning colors.
- Large, lightweight, borderless search input at the top; compact icon/text filters and icon-only window actions below it.
- History cards are transparent at rest, use `--hover-bg` on hover/focus, and mix selection color into the selected+checked state. Do not turn every row into a permanently elevated card.
- Card content hierarchy: title 闂?optional preview 闂?source/time/actions metadata. Keep truncation, compact mode, and virtual-scroll measurement aligned.
- Status bar is a low-contrast footer separated by one subtle border; popovers/context surfaces are elevated with a border plus shadow.
- Motion is short and functional. Respect the global reduced-motion rule.

Do not broadly restyle the main page as part of a settings task. Inspect it to borrow the language, not to make opportunistic changes.

## Theme color contract

The current `ThemeColors` interface has 20 semantic values:

| CSS variable          | ThemeColors key    | Purpose                                           |
| --------------------- | ------------------ | ------------------------------------------------- |
| `--bg-app`            | `bg`               | application/body and main-window shell background |
| `--bg-settings`       | `settingsBg`       | settings window shell background                  |
| `--accent`            | `accent`           | product/focus accent                              |
| `--text-primary`      | `textPrimary`      | primary content                                   |
| `--text-secondary`    | `textSecondary`    | secondary content                                 |
| `--text-muted`        | `textMuted`        | descriptions/metadata                             |
| `--text-faint`        | `textFaint`        | lowest-emphasis text/icons                        |
| `--placeholder-color` | `placeholderColor` | placeholders                                      |
| `--border-color`      | `border`           | regular borders                                   |
| `--border-subtle`     | `borderSubtle`     | dividers/quiet borders                            |
| `--card-bg`           | `cardBg`           | card/elevated controls                            |
| `--surface-bg`        | `surfaceBg`        | popovers/panels                                   |
| `--statusbar-bg`      | `statusBarBg`      | footer/status bar                                 |
| `--hover-bg`          | `hoverBg`          | hover and quiet selected surfaces                 |
| `--input-bg`          | `inputBg`          | inputs and inset surfaces                         |
| `--selection-color`   | `selectionColor`   | selection/current state                           |
| `--success-color`     | `successColor`     | successful state                                  |
| `--danger-color`      | `dangerColor`      | destructive/error state                           |
| `--warning-color`     | `warningColor`     | caution/favorite emphasis                         |
| `--scrollbar-color`   | `scrollbarColor`   | scroll thumb                                      |

Use these variables for reusable surfaces, text, borders, controls, and status states. Derive translucency with `color-mix` instead of inventing parallel shades.

Literal colors are acceptable only for content-defined rendering where theme semantics are not the source of meaning, such as syntax token colors, media backdrops, forced-colors overrides, or intentionally fixed source/category tones. Those cases are niche and must be reviewed/documented rather than generalized.

The optional color-icon mode (`general.colorIcons`) is an intentional fixed-palette exception: the built-in palette `DEFAULT_ICON_COLORS` in `src/lib/types/clipboard.ts` gives each icon a fixed hex color, with optional per-icon overrides in `general.iconColors`. Colors are applied only when the user enables the setting; the default remains `currentColor` so the approved monochrome language is preserved. See `niche_ui_style.md` for the review question on whether these tones should become semantic.

## Global font and display variables

| Variable                  | Current default | Use                     |
| ------------------------- | --------------- | ----------------------- |
| `--font-size-base`        | `14px`          | general UI/body         |
| `--font-size-secondary`   | `11px`          | metadata/descriptions   |
| `--font-size-tiny`        | `10px`          | smallest notes          |
| `--font-size-cardTitle`   | `13px`          | card title              |
| `--font-size-cardPreview` | `11px`          | card preview            |
| `--show-secondary`        | `block`/`none`  | display preference hook |

`settings-bootstrap.ts` applies these and the 20 theme variables at startup. Live settings panels must update the same contract; do not create alternate variable names for the same meaning.

## Settings semantic metrics

`StorageSettingsDialog.svelte` defines the settings scope variables consumed by shell and child panels:

| Variable                                                                           | Standard fallback/use                           |
| ---------------------------------------------------------------------------------- | ----------------------------------------------- |
| `--settings-page-title-size`                                                       | base + 4px; standalone panel `h2`               |
| `--settings-heading-size`                                                          | base - 1px; card/section heading                |
| `--settings-description-size`                                                      | secondary size; descriptions and breadcrumb     |
| `--settings-note-size`                                                             | tiny size; notes/counts                         |
| `--settings-control-size`                                                          | secondary size; buttons/inputs/select/list rows |
| `--settings-feedback-size`                                                         | description size                                |
| `--settings-feedback-radius`                                                       | 7px                                             |
| `--settings-card-radius`                                                           | 9px                                             |
| `--settings-control-radius`                                                        | 6px                                             |
| `--settings-icon-radius`                                                           | 7px                                             |
| `--settings-close-size` / `--settings-close-radius` / `--settings-close-font-size` | 28px / 7px / 19px                               |

Use a semantic setting variable when it fits. A raw metric is acceptable for a genuinely component-specific geometry (for example a color swatch or circular knob), not as a second spelling of an existing setting token.

## Shared settings primitives

`settings-shared.css` currently owns:

- standalone `header`, `.eyebrow`, `h2`, header description, and `.close-button`;
- `.settings-scroll` scroll container;
- `.setting-card`, `.toggle-card`, `.setting-heading`, `.setting-icon`, `.heading-inline`, `.value-label`;
- `.toggle-switch`, `.toggle-knob` and active/disabled states;
- `.popover-surface` shared dropdown popover visual primitive (used by both settings and the main page): `z-index: 100`, `width: max-content`, `max-width: calc(100vw - 16px)`, `1px var(--border-color)` border, `8px` radius, `--surface-bg`, `0 8px 28px` shadow; plus shared `button`/`:hover`/`.selected`/`:disabled` states (`--text-secondary`/`--text-primary`, `--hover-bg`, `--selection-color` + 8% mix, `7px 12px` padding). Option buttons wrap their label in a `<span>` that fills the button's content box (`flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis`) and align via `text-align`: `center` by default, `right` when the list-level `.text-overflow` class is present. Overflowing labels are truncated by `alignDropdownOptionText()` (`src/lib/utils/dropdown.ts`): it measures each label span's `scrollWidth` against `clientWidth` (falling back to the button), cuts the text at a character boundary to fit and appends `闁炽儺娉? then the list-level `text-align: right`makes the truncated result flush right exactly like the fitting options - no hard clipping, so no half character ever shows at the cut point. The full label is stored in the button's`title`for hover. The function runs on open and on option-list changes (both settings`CustomSelect`and main-page dropdowns); spans are re-created per open, so truncation always starts from the full text. The DatePicker calendar popover does not call it, so its grid/footer buttons (direct text nodes, no label spans) are unaffected. Because`width: max-content`is what sizes the popover to its longest item, do not force an inline`width` on the popover (it would override the natural size and freeze the width at a stale value). Dropdown popover widths stay content-based (`width: max-content`), capped at `150px`- the truncation threshold beyond which`alignDropdownOptionText()`cuts labels - so short lists (e.g. a two-character date option) render at their natural width instead of stretching. Each surface keeps its own positioning: settings`.custom-select-popover`adds`position: fixed`with JS-computed`top`/`left`plus an inline`min-width`=`max(trigger width, content width)`capped at`150px`(so the list never collapses below its trigger),`max-width: 150px`, `max-height: 240px`, `overflow-x: hidden`+`overflow-y: auto`; the main-page `.dropdown-popover`(source app/date filters) keeps`position: absolute; top: calc(100% + 4px); right: 0; min-width: 100%; max-width: 150px; overflow: hidden` on its relative wrapper, so the list's minimum width equals the trigger button's FIXED width (`width: 80px`on`.filter-dropdown-btn`; the label truncates with ellipsis via `.dropdown-label { min-width: 0; max-width: 80px; overflow: hidden; text-overflow: ellipsis }`) - the list never renders narrower than the button, short lists are not stretched beyond the button width, and 150px remains the cap where truncation kicks in. Because`width: max-content`sizes the popover to its longest item, do not force an inline`width`or a CSS`min-width` of 150px on the popover - it would freeze the natural size and stretch short lists.
- `.settings-select` shared control/button base (input look: custom arrow, hover/focus/disabled states). Settings dropdowns must use the `CustomSelect.svelte` component, not native `<select>`/`<option>`; the trigger keeps the closed `.settings-select` look, opening into the shared `.popover-surface` described above.
- `.transparency-slider` including WebKit and Firefox tracks/thumbs;
- `.settings-feedback` success/error states;
- `.settings-state` loading/unavailable placeholder state (centered muted text) and `.auto-save-note` plus the default pointer cursor for buttons;
- `.restart-note` for restart-required notices in general-settings cards.

`src/app.css` imports this file globally. New child panels must rely on these primitives and add only their panel-specific layout. `CompactSettingsPanel.svelte` is the cleanest minimal example. `GeneralSettingsPanel.svelte`, `FontSizeSettingsPanel.svelte`, `ThemeSettingsPanel.svelte`, and `KeyboardSettingsPanel.svelte` demonstrate scoped extensions.

Legacy/local duplication exists in the settings shell and ignored-app panel; it is recorded in `niche_ui_style.md` and is not a license to copy shared rules.

## CSS ownership decision

Before adding a rule, place it at the narrowest correct stable level:

- Theme color or global accessibility/reset 闂?`app.css`, `ThemeColors`, presets, and `theme.ts` together.
- Global scrollbar treatment (thin width, themed thumb via `--scrollbar-color`) 闂?`app.css` on the universal `*` selectors; components only add scrollability, never per-scope scrollbar rules. The `.filters` row is the intentional exception (hidden scrollbar).
- Shared settings card/control/feedback primitive 闂?`settings-shared.css`.
- Settings navigation/shell/built-in storage-OCR-statistics layout 闂?`StorageSettingsDialog.svelte`.
- One reusable component's unique layout 闂?that component's scoped `<style>`.
- One-off content visualization 闂?scoped style plus a note in `niche_ui_style.md` if it does not follow the general theme contract.

Do not use a parent scoped selector to style inside a child Svelte component. Pass props/classes or move the shared rule into a global/shared stylesheet.

## Form and control conventions

- Toggle card: label/icon/description on the left, switch or compact control on the right in one row.
- Slider card: heading/value on one row, unwrapped `input[type="range"].transparency-slider` below, and `--slider-pct` updated from the current value.
- Number input: use textfield appearance and hide WebKit spin buttons.
- Select: remove the native arrow only when the replacement affordance and theme behavior are deliberately handled; use settings control size/radius/colors.
- Feedback: use `.settings-feedback` with `.success`; keep it dismissible by time and accessible through appropriate live/status semantics.
- Buttons and inputs need visible focus. Never remove outline without a replacement.

## Fixed layer order

| Layer                         | Current z-index |
| ----------------------------- | --------------- |
| settings backdrop             | 50              |
| detail backdrop/panel         | 51 / 52         |
| image viewer overlay/controls | 200 / 201       |
| main search suggestion panel  | 110             |
| context menu                  | 9999            |

Dropdown popovers use a local stacking context around 100. Check the complete stacking context before changing a value; do not solve one overlap by arbitrary escalation.

## Style change gate

1. Identify whether the task changes the main page, settings, or a niche surface.
2. Inspect the target plus sibling components that use the same primitive.
3. Confirm token/ownership placement using this reference.
4. Search for duplicate selectors and raw colors/metrics before adding new declarations.
5. Keep markup hierarchy, keyboard focus, overflow, narrow-window behavior, and reduced/high-contrast behavior intact.
6. Run `npm run check` and `npm run build`.
7. Perform rendered/runtime comparison in dark and light/custom theme when the change affects theme-facing UI; test the target window size and a narrow size.
8. Record a new niche exception in `niche_ui_style.md` rather than treating it as a general rule.
9. Run the documentation currency gate before commit.
