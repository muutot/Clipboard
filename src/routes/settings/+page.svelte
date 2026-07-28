<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import StorageSettingsDialog from "$lib/components/StorageSettingsDialog.svelte";
  import { generalSettings } from "$lib/services/settings";
  import { applyGeneralSettingsToDocument } from "$lib/services/settings-bootstrap";

  let s = $state($generalSettings);
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
      applyGeneralSettingsToDocument(v);
    });
    return unsub;
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
