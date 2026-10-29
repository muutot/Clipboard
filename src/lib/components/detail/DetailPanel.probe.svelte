<script module lang="ts">
  import type { ClipboardItem } from "$lib/types/clipboard";
  export interface DetailProbeState {
    setItem?: (item: ClipboardItem | null) => void;
  }
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import DetailPanel from "./DetailPanel.svelte";
  let {
    out,
    initial,
    save,
  }: {
    out: DetailProbeState;
    initial: ClipboardItem;
    save: (id: string, content: string) => Promise<boolean>;
  } = $props();
  let item = $state.raw<ClipboardItem | null>(initial);
  untrack(
    () =>
      (out.setItem = (next) => {
        item = next;
      }),
  );
</script>

<DetailPanel
  {item}
  mode="split"
  onclose={() => {}}
  oncopy={() => {}}
  onedit={() => {}}
  onsaveedit={save}
  onrenametitle={() => {}}
  onplainpaste={() => {}}
  onformatpaste={() => {}}
  oncleanpaste={() => {}}
  onduplicate={() => {}}
  onsaveasnew={() => {}}
  oncopyfilename={() => {}}
  oncopyPath={() => {}}
  onsavetags={() => {}}
  onocrupdate={() => {}}
/>
