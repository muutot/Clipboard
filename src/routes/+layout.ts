// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import { get } from "svelte/store";
import { generalSettings } from "$lib/services/settings";
import { loadOsLocale, setLocale } from "$lib/i18n";

export const ssr = false;

export async function load() {
  // Both must settle before the first render: the settings store hydrates the
  // `language` setting, and the OS locale must be in place before a
  // `system` language is resolved — the webview language alone can open a
  // Chinese system in English.
  await Promise.all([generalSettings.initialize(), loadOsLocale()]);
  const settings = get(generalSettings);
  if (settings.language === "system") setLocale(settings.language);
  return {};
}
