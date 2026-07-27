<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import StorageSettingsDialog from "$lib/components/StorageSettingsDialog.svelte";
  import { generalSettings } from "$lib/services/settings";
  import { applyThemeColors } from "$lib/utils/theme";

  let s = $state($generalSettings);
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
      if (v.themeColors) {
        applyThemeColors(v.themeColors);
      }
    });
    return unsub;
  });

  $effect(() => {
    const r = document.documentElement.style;
    r.fontSize = `${s.fontSizes.base}px`;
    r.setProperty("--font-size-base", `${s.fontSizes.base}px`);
    r.setProperty("--font-size-secondary", `${s.fontSizes.secondary}px`);
    r.setProperty("--font-size-tiny", `${s.fontSizes.tiny}px`);
    r.setProperty("--font-size-cardTitle", `${s.fontSizes.cardTitle}px`);
    r.setProperty("--font-size-cardPreview", `${s.fontSizes.cardPreview}px`);
    r.setProperty(
      "--show-secondary",
      s.display.showSecondaryText ? "block" : "none",
    );
  });

  function handleClose() {
    getCurrentWindow().close();
  }
</script>

<div class="settings-shell">
  <StorageSettingsDialog open={true} onclose={handleClose} standalone={true} />
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--bg-settings, #1b1b1b);
  }

  .settings-shell {
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: var(--bg-settings, #1b1b1b);
  }
</style>
