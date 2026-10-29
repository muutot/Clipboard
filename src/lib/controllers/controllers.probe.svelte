<script module lang="ts">
  import { createHistoryController } from "./history.svelte";
  import { createSearchController } from "./search.svelte";
  import { createBulkController } from "./bulk.svelte";
  import { createItemStoreView, type ItemStoreView } from "$lib/utils/store/item-store-view.svelte";
  export interface ProbeState {
    store?: ItemStoreView;
    history?: ReturnType<typeof createHistoryController>;
    search?: ReturnType<typeof createSearchController>;
    bulk?: ReturnType<typeof createBulkController>;
    setQuery?: (value: string) => void;
    removeCanonical?: (ids: ReadonlySet<string>) => void;
  }
</script>

<script lang="ts">
  import { untrack, onDestroy } from "svelte";
  import { createItemMutations } from "$lib/utils/store/item-mutations";
  import {
    createItemStore,
    captureAffectedItems,
    restoreAffectedItems,
    createRemovalRollbackGuard,
    removeItems,
  } from "$lib/utils/store/item-store";
  import { DEFAULT_GENERAL_SETTINGS } from "$lib/services/settings";
  import type { ClipboardItem } from "$lib/types/clipboard";
  let { out, initial = [] }: { out: ProbeState; initial?: ClipboardItem[] } = $props();
  let mutations: ReturnType<typeof createItemMutations> | undefined;
  const store = untrack(() =>
    createItemStoreView(
      createItemStore(initial),
      (before, next) => mutations?.observe(before, next) ?? next,
    ),
  );
  mutations = createItemMutations(() => store);
  onDestroy(() => mutations?.destroy());
  const removalRollbacks = createRemovalRollbackGuard();
  onDestroy(() => removalRollbacks.destroy());
  let query = $state("");
  const settings = {
    ...DEFAULT_GENERAL_SETTINGS,
    display: { ...DEFAULT_GENERAL_SETTINGS.display, pageSize: 1 },
  };
  const history = createHistoryController({
    itemStore: store,
    settings,
    filter: () => ({}),
    onError: () => {},
    acceptRecords: (records) => mutations!.acceptRecords(records),
  });
  const search = createSearchController({
    itemStore: store,
    settings,
    get query() {
      return query;
    },
    isDeleted: false,
    filter: () => ({}),
    status: "",
    translate: (key) => key,
    flushSettings: async () => {},
    rememberSearchTerm: () => {},
    acceptRecords: (records) => mutations!.acceptRecords(records),
  });
  const bulk = createBulkController({
    itemStore: store,
    metadataMutations: mutations,
    settings,
    history,
    // Mirror the route: selection order (insertion-ordered set) drives the
    // derived items, not list order.
    get selectedLoadedItems() {
      return [...bulk.selectedIds]
        .map((id) => store.history.find((entry) => entry.id === id))
        .filter((item): item is ClipboardItem => item !== undefined);
    },
    get allSelectedFavorites() {
      return store.history.filter((i) => bulk.selectedIds.has(i.id)).every((i) => i.favorite);
    },
    captureAffected: (ids, options) =>
      removalRollbacks.track(captureAffectedItems(store.current, ids, bulk.selectedIds, options)),
    rollbackAffected: (snapshot) => {
      store.current = restoreAffectedItems(store.current, snapshot);
    },
    finishAffected: (snapshot) => removalRollbacks.finish(snapshot),
    statusMessage: "",
    translate: (key) => key,
  });
  untrack(() =>
    Object.assign(out, {
      store,
      history,
      search,
      bulk,
      removeCanonical: (ids: ReadonlySet<string>) => {
        removalRollbacks.invalidate(ids);
        store.current = removeItems(store.current, ids);
      },
      setQuery: (value: string) => {
        query = value;
      },
    }),
  );
</script>

<div>{store.history.length} / {store.indexed?.length ?? 0}</div>
