<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import "../app.css";
  import "$lib/styles/settings-shared.css";
  import { generalSettings } from "$lib/services/settings";
  import {
    applyGeneralSettingsToDocument,
    syncCompactShellClass,
  } from "$lib/services/settings-bootstrap";

  let { children } = $props();

  if (typeof document !== "undefined") {
    applyGeneralSettingsToDocument(get(generalSettings));
  }

  onMount(() => {
    syncCompactShellClass(get(generalSettings).compactMode);
  });
</script>

{@render children()}
