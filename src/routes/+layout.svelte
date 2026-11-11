<script lang="ts">
  import { get } from "svelte/store";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import "../app.css";
  import "#lib/styles/settings-shared.css";
  import { generalSettings } from "#lib/services/settings.ts";
  import { destroySourceMappingRules } from "#lib/services/source-mapping.svelte.ts";
  import { applyGeneralSettingsToDocument } from "#lib/services/settings-bootstrap.ts";
  import { isTauriRuntime } from "#lib/services/runtime.ts";
  import { isCustomCssRecoveryShortcut } from "#lib/utils/settings/custom-css.ts";
  import { watchSystemTheme } from "#lib/utils/settings/theme.ts";
  import AppContextMenu from "#lib/components/common/AppContextMenu.svelte";
  import { onDestroy, onMount } from "svelte";

  let { children } = $props();

  onDestroy(destroySourceMappingRules);

  onMount(() => {
    const settings = get(generalSettings);
    // Apply here instead of at module scope: module init runs before the load
    // gate resolves, so it would paint (and cache) the dark defaults first.
    applyGeneralSettingsToDocument(settings);

    // The main window is created hidden (tauri.conf.json). Showing it from the
    // page (after `load()` hydrated the settings and Svelte rendered) keeps the
    // first visible frame on the configured theme instead of the startup
    // defaults.
    if (isTauriRuntime() && getCurrentWindow().label === "main") {
      void invoke("show_main_window_command").catch((error) =>
        console.error("Unable to show the main window", error),
      );
    }
    // Re-apply on every store change so settings edited in the settings window
    // (persisted → broadcast via "general-settings-changed" → applyRemote) reach
    // this window live; window opacity and theme colors are document-level CSS
    // variables that would otherwise stay at their startup values. The bootstrap
    // call is idempotent and already covers custom CSS.
    const unsubscribe = generalSettings.subscribe((settings) =>
      applyGeneralSettingsToDocument(settings),
    );
    const stopSystemTheme = watchSystemTheme(() => {
      const current = get(generalSettings);
      if (current.theme === "system") applyGeneralSettingsToDocument(current);
    });
    const recover = (event: KeyboardEvent) => {
      if (!get(generalSettings).customCssEnabled || !isCustomCssRecoveryShortcut(event)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      generalSettings.updateSetting("customCssEnabled", false);
      void generalSettings
        .flush()
        .catch(() => console.error("Unable to persist custom CSS recovery"));
    };
    window.addEventListener("keydown", recover, true);
    return () => {
      unsubscribe();
      stopSystemTheme();
      window.removeEventListener("keydown", recover, true);
    };
  });
</script>

{@render children()}

<AppContextMenu />
