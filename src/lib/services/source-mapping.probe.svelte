<script lang="ts">
  import type { SourceMappingStore } from "./source-mapping.svelte";

  let {
    store,
    item,
  }: {
    store: SourceMappingStore;
    item: Parameters<SourceMappingStore["resolveWebSourceName"]>[0];
  } = $props();

  // Match the card's owned derived name rather than testing only direct reads.
  const name = $derived(store.resolveWebSourceName(item));
</script>

<span>{name}</span>
