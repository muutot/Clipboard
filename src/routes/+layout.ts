// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import { get } from "svelte/store";
import { generalSettings } from "$lib/services/settings";
import { ensureSourceMappingRules } from "$lib/services/source-mapping.svelte";
import { loadOsLocale, setLocale } from "$lib/i18n";

export const ssr = false;

export async function load() {
  // Hydrate display settings and source names before the first render. The
  // OS locale must also be ready before resolving a `system` language: the
  // webview language alone can open a Chinese system in English.
  await Promise.all([generalSettings.initialize(), loadOsLocale(), ensureSourceMappingRules()]);
  const settings = get(generalSettings);
  if (settings.language === "system") setLocale(settings.language);
  return {};
}
