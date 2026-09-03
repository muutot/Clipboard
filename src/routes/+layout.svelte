<script lang="ts">
  import { get } from "svelte/store";
  import "../app.css";
  import "$lib/styles/settings-shared.css";
  import { generalSettings } from "$lib/services/settings";
  import { applyGeneralSettingsToDocument } from "$lib/services/settings-bootstrap";

  let { children } = $props();

  if (typeof document !== "undefined") {
    applyGeneralSettingsToDocument(get(generalSettings));
  }
</script>

{@render children()}
