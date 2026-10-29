<script lang="ts">
  import { get } from "svelte/store";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import "../app.css";
  import "$lib/styles/settings-shared.css";
  import { generalSettings } from "$lib/services/settings";
  import { destroySourceMappingRules } from "$lib/services/source-mapping.svelte";
  import { applyGeneralSettingsToDocument } from "$lib/services/settings-bootstrap";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { applyCustomCss, isCustomCssRecoveryShortcut } from "$lib/utils/settings/custom-css";
  import AppContextMenu from "$lib/components/common/AppContextMenu.svelte";
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
    const unsubscribe = generalSettings.subscribe((settings) =>
      applyCustomCss(settings.customCss, settings.customCssEnabled),
    );
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
      window.removeEventListener("keydown", recover, true);
    };
  });
</script>

{@render children()}

<AppContextMenu />
