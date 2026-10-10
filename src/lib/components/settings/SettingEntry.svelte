<script lang="ts">
  import type { SettingEntryConfig } from "$lib/types/settings-entry";
  import type { Snippet } from "svelte";
  import CustomEntry from "./settings-entries/CustomEntry.svelte";
  import HeadingEntry from "./settings-entries/HeadingEntry.svelte";
  import NumberEntry from "./settings-entries/NumberEntry.svelte";
  import SelectEntry from "./settings-entries/SelectEntry.svelte";
  import SizeEntry from "./settings-entries/SizeEntry.svelte";
  import SliderEntry from "./settings-entries/SliderEntry.svelte";
  import TextEntry from "./settings-entries/TextEntry.svelte";
  import ToggleEntry from "./settings-entries/ToggleEntry.svelte";

  interface Props {
    config: SettingEntryConfig;
    children?: Snippet;
  }

  let { config, children }: Props = $props();
</script>

{#if config.type === "heading"}
  <HeadingEntry {config} />
{:else if config.type === "slider"}
  <SliderEntry {config} />
{:else if config.type === "toggle"}
  <ToggleEntry {config} />
{:else if config.type === "select"}
  <SelectEntry {config} />
{:else if config.type === "text"}
  <TextEntry {config} />
{:else if config.type === "size"}
  <SizeEntry {config} />
{:else if config.type === "custom"}
  <CustomEntry {config} {children} />
{:else}
  <NumberEntry {config} />
{/if}
