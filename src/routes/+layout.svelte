<script lang="ts">
  import { get } from "svelte/store";
  import "../app.css";
  import "$lib/styles/settings-shared.css";
  import { generalSettings } from "$lib/services/settings";
  import { applyGeneralSettingsToDocument } from "$lib/services/settings-bootstrap";
  import { applyCustomCss, isCustomCssRecoveryShortcut } from "$lib/utils/custom-css";
  import { onMount } from "svelte";

  let { children } = $props();

  if (typeof document !== "undefined") {
    applyGeneralSettingsToDocument(get(generalSettings));
  }

  onMount(() => {
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
