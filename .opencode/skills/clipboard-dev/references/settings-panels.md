# Settings Panels — Detailed Reference

## Panel Convention

All settings panels follow this pattern:
```svelte
<script lang="ts">
  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }
  let { onclose, showHeader = true }: Props = $props();
</script>
```

When rendered inside `StorageSettingsDialog`, child panels receive `showHeader={false}`.

## State Management Pattern

```typescript
// Read from global store
let localSettings = $state($generalSettings);

// Sync from store when it changes
$effect(() => {
  localSettings = $generalSettings;
});

// Save changes
function updateSettings() {
  persistGeneralSettings(localSettings);
}
```

## Feedback Toast Pattern

```typescript
let feedback = $state('');
let feedbackSuccess = $state(false);

function showFeedback(msg: string, success: boolean) {
  feedback = msg;
  feedbackSuccess = success;
  setTimeout(() => { feedback = ''; }, 2000);
}
```

Uses `.feedback-banner` / `.feedback-banner-success` CSS classes from `settings-shared.css`.

## Slider Handler Pattern

```typescript
const sliderHandler = (key: string) => (event: Event) => {
  const target = event.target as HTMLInputElement;
  const value = parseInt(target.value, 10);
  if (!isNaN(value)) {
    settings[key] = value;
    updateSettings();
  }
};
```

## Panel List

| Panel | Props | Sections |
|---|---|---|
| `GeneralSettingsPanel` | `section: "search" \| "items" \| "display" \| "window"` | Sort rules, search filters, display options, window config |
| `FontSizeSettingsPanel` | — | Subnav: "interface" (5 sliders) / "card" (4 sliders) |
| `ThemeSettingsPanel` | — | Theme mode switch (dark/light/custom), color pickers, preset CRUD |
| `CompactSettingsPanel` | — | Compact mode toggle + 9 dimension sliders |
| `KeyboardSettingsPanel` | `category: "item" \| "clipboard" \| "system"` | Shortcut recording, reset functionality |
| `IgnoredAppsSettingsPanel` | `iconsDir?: string` | Two-column transfer list, privacy pause toggle |

## StorageSettingsDialog Shell

The parent dialog owns the navigation hierarchy:
1. Breadcrumb: `设置 / {一级分组}`
2. Secondary-group tabs (multiple sections) or current section label (single section)
3. Description line
4. Setting cards

Child panels must NOT render their own header when `showHeader={false}`.
