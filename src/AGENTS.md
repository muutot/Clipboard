# src/ — Svelte 5 frontend

Read `skills/clipboard-dev/SKILL.md` first. Before editing a subsystem, read its reference:

| Touching                          | Read                                                                  |
| --------------------------------- | --------------------------------------------------------------------- |
| Any component                     | `skills/clipboard-dev/references/components.md`                       |
| Services, stores, invoke wrappers | `skills/clipboard-dev/references/services.md`                         |
| Any CSS or markup                 | `skills/clipboard-dev/references/css-theming.md`                      |
| Settings shell, panels, controls  | `skills/clipboard-dev/references/settings-panels.md` + css-theming.md |
| Search pagination / cache         | `skills/clipboard-dev/references/search-cache-strategy.md`            |
| Hotkeys, keyboard dispatch        | `skills/clipboard-dev/references/hotkey-architecture.md`              |

Hard rules:

- The approved main-page visual language is preserved; do not redesign it in unrelated tasks.
- i18n contract: `src/lib/i18n/locales/en.ts`, `src/lib/i18n/locales/zh-CN.ts`, and
  `src/lib/i18n/types.ts` change together.
- A control bound to backend-loaded settings must hydrate before it renders (shared
  module-level store or a load gate). Settings panels are lazy-loaded and destroyed on every
  section switch — a per-mount fetch flashes wrong values.
- Settings window components report results via the `onfeedback` callback, not `showToast`
  (the toast host exists only in the main and float windows).
- Settings search entries need explicit `id` attributes matching the indices in
  `src/lib/settings-search.ts`.
- Clipboard list loading uses the stale-while-revalidate pattern with requestId guards;
  do not replace it with plain refetches.
- Error effects in detail panels must be guarded with `$derived(item.id)`.
