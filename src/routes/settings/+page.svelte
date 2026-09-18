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

  async function handleClose() {
    // Flush the debounced settings write before destroying the webview:
    // otherwise an edit made within the persist debounce window is lost.
    // The window still closes when persistence fails (the pending value is
    // retained for retry); trapping the user here would be worse.
    try {
      await generalSettings.flush();
    } catch (error) {
      console.error("Unable to persist settings before close:", error);
    }
    await getCurrentWindow().close();
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
