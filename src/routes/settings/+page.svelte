<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import StorageSettingsDialog from "$lib/components/settings/StorageSettingsDialog.svelte";
  import ConfirmDialog from "$lib/components/common/ConfirmDialog.svelte";
  import { generalSettings } from "$lib/services/settings";
  import { applyGeneralSettingsToDocument } from "$lib/services/settings-bootstrap";
  import { isTauriRuntime, isDevtoolsHotkey, openDevtools } from "$lib/services/runtime";

  function handleDevtoolsKeydown(event: KeyboardEvent) {
    if (isDevtoolsHotkey(event)) {
      event.preventDefault();
      openDevtools();
    }
  }

  let s = $state($generalSettings);
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
      applyGeneralSettingsToDocument(v);
    });
    return unsub;
  });

  onMount(() => {
    if (!isTauriRuntime()) return;
    let revealed = false;
    const win = getCurrentWindow();
    const reveal = async () => {
      if (revealed) return;
      revealed = true;
      applyGeneralSettingsToDocument(get(generalSettings));
      await win.show();
      await win.setFocus();
    };
    void generalSettings.initialize().finally(() => void reveal());
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

<svelte:window onkeydown={handleDevtoolsKeydown} />

<div class="settings-shell">
  <StorageSettingsDialog open={true} onclose={handleClose} standalone={true} />
  <ConfirmDialog />
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
