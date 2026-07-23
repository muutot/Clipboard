<script lang="ts">
  import StorageSettingsDialog from "$lib/components/StorageSettingsDialog.svelte";

  function handleClose() {
    if ('__TAURI_INTERNALS__' in window) {
      import("@tauri-apps/api/window").then(({ getCurrentWindow }) => {
        getCurrentWindow().close();
      });
    }
  }
</script>

<div class="settings-shell">
  <StorageSettingsDialog open={true} onclose={handleClose} />
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #1b1b1b;
  }

  .settings-shell {
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: #1b1b1b;
  }
</style>
