<script lang="ts">
  // Test-only probe for `item-store-view.svelte`.
  //
  // The view has to be created *inside* a component: a `$derived` declared
  // outside any reaction is unowned, stops tracking its dependencies, and goes
  // stale — which is why the route creates the view during its own init and why
  // this probe does the same instead of letting the test build it.
  import { untrack } from "svelte";
  import type { ItemStoreView } from "./item-store-view.svelte";
  import { createItemStoreView } from "./item-store-view.svelte";
  import type { ItemStore } from "./item-store";

  let {
    initial,
    out,
    onchange,
  }: {
    initial: ItemStore;
    /** Handed back to the test so it can advance the store. */
    out: { view?: ItemStoreView };
    onchange?: (snapshot: {
      historyIds: string[];
      indexedIds: string[] | null;
      detailId: string | null;
      /** True when a projected record is the very object the store holds. */
      identityHeld: boolean;
    }) => void;
  } = $props();

  // The seed is read once, on purpose: the probe mirrors a route that builds its
  // store during init, and `untrack` says so instead of leaving it to look like
  // a missed dependency.
  const view = untrack(() => createItemStoreView(initial));
  untrack(() => (out.view = view));

  $effect(() => {
    const first = view.history[0];
    onchange?.({
      historyIds: view.history.map((item) => item.id),
      indexedIds: view.indexed?.map((item) => item.id) ?? null,
      detailId: view.detail?.id ?? null,
      identityHeld: first !== undefined && first === view.current.byId.get(first.id),
    });
  });
</script>

<div data-testid="probe">
  {view.history.length}|{view.indexed?.length ?? -1}|{view.detail?.id ?? "-"}
</div>
