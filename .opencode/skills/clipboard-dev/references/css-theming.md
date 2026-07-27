# CSS & Theming — Detailed Reference

## Theme Color Variables

Defined in `src/lib/utils/theme.ts` and `DARK_THEME_COLORS` in `src/lib/types/clipboard.ts`:

| Variable | Dark Default | Description |
|---|---|---|
| `--bg-app` | `#0e0e10` | App background |
| `--bg-settings` | `#18181b` | Settings background |
| `--accent` / `--selection-color` | `#4aa8ff` | Primary accent/selection |
| `--text-primary` | `#e4e4e7` | Primary text |
| `--text-secondary` | `#a1a1aa` | Secondary text |
| `--text-muted` | `#71717a` | Muted text |
| `--text-faint` | `#52525b` | Faint text |
| `--border-color` | `#27272a` | Default border |
| `--border-subtle` | `#1f1f23` | Subtle border |
| `--card-bg` | `#1c1c1f` | Card background |
| `--surface-bg` | `#18181b` | Surface background |
| `--statusbar-bg` | `#111113` | Status bar background |
| `--hover-bg` | `#2a2a2d` | Hover background |
| `--input-bg` | `#1a1a1d` | Input background |
| `--success-color` | `#22c55e` | Success indicator |
| `--danger-color` | `#ef4444` | Danger indicator |
| `--warning-color` | `#f59e0b` | Warning indicator |
| `--scrollbar-color` | `#424245` | Scrollbar thumb |

## app.css Variables

Additional variables defined in `src/app.css`:

| Variable | Description |
|---|---|
| `--clr-bg-primary` | Primary background |
| `--clr-bg-secondary` | Secondary background |
| `--clr-bg-elevated` | Elevated surface |
| `--clr-surface` | Surface |
| `--clr-surface-hover` | Surface hover |
| `--clr-surface-active` | Surface active |
| `--clr-border` | Border |
| `--clr-border-subtle` | Subtle border |
| `--clr-text-primary` | Primary text |
| `--clr-text-secondary` | Secondary text |
| `--clr-text-tertiary` | Tertiary text |
| `--clr-accent` | Accent |
| `--clr-accent-hover` | Accent hover |
| `--clr-accent-active` | Accent active |
| `--clr-danger` | Danger |
| `--clr-danger-hover` | Danger hover |
| `--clr-success` | Success |
| `--clr-warning` | Warning |
| `--clr-info` | Info |
| `--font-size-general` | General font size |
| `--font-size-card` | Card font size |
| `--font-size-detail` | Detail font size |
| `--font-size-menu` | Menu font size |
| `--font-size-badge` | Badge font size |

## Settings Typography Variables

Defined on `StorageSettingsDialog.svelte` shell, used by all panels:

| Variable | Fallback | Usage |
|---|---|---|
| `--settings-page-title-size` | `18px` | Panel `h2` |
| `--settings-heading-size` | `13px` | `.setting-heading strong` |
| `--settings-description-size` | `var(--font-size-secondary, 11px)` | Descriptions, hints, breadcrumb |
| `--settings-note-size` | `var(--font-size-tiny, 10px)` | Auto-save notes, footnotes |
| `--settings-control-size` | `var(--font-size-secondary, 11px)` | Inputs, selects, buttons, list rows |
| `--settings-feedback-size` | description size | Feedback toast |
| `--settings-feedback-radius` | `7px` | Feedback toast |
| `--settings-card-radius` | `9px` | `.setting-card` |
| `--settings-control-radius` | `6px` | Inputs, selects, small buttons |
| `--settings-close-size` | `28px` | Close button |
| `--settings-close-radius` | `7px` | Close button |
| `--settings-close-font-size` | `19px` | Close button |
| `--settings-icon-radius` | `7px` | `.setting-icon` border-radius |

## settings-shared.css Classes

Provides these shared classes for all settings panels:
- `.header`, `.eyebrow`, `h2`, `.close-button`
- `.settings-scroll`
- `.action-card`, `.action-card-compact`
- `.toggle-track`, `.toggle-thumb`
- `.select-control`, `.slider-control`
- `.btn-primary`, `.btn-outline`
- `.feedback-banner`, `.feedback-banner-success`
- `button { cursor: pointer; }` global rule

## Theme Application Flow

1. `settings-bootstrap.ts` → `applyGeneralSettingsToDocument()` sets CSS vars on `document.documentElement`
2. `theme.ts` → `applyThemeColors(colors?)` sets 19 CSS custom properties on `document.documentElement.style`
3. `DARK_THEME_COLORS` provides dark mode defaults, `LIGHT_THEME_COLORS` for light mode
4. Custom themes use `ThemePreset` objects with `ThemeColors` (12+ color properties)
