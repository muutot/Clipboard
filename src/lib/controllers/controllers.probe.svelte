<script module lang="ts">
  import { createHistoryController } from "./history.svelte";
  import { createSearchController } from "./search.svelte";
  import { createBulkController } from "./bulk.svelte";
  import { createItemStoreView, type ItemStoreView } from "$lib/utils/item-store-view.svelte";
  export interface ProbeState {
    store?: ItemStoreView;
    history?: ReturnType<typeof createHistoryController>;
    search?: ReturnType<typeof createSearchController>;
    bulk?: ReturnType<typeof createBulkController>;
    setQuery?: (value: string) => void;
  }
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import {
    createItemStore,
    captureAffectedItems,
    restoreAffectedItems,
  } from "$lib/utils/item-store";
  import { DEFAULT_GENERAL_SETTINGS } from "$lib/services/settings";
  import type { ClipboardItem } from "$lib/types/clipboard";
  let { out, initial = [] }: { out: ProbeState; initial?: ClipboardItem[] } = $props();
  const store = untrack(() => createItemStoreView(createItemStore(initial)));
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
  });
  const bulk = createBulkController({
    itemStore: store,
    settings,
    history,
    get selectedLoadedItems() {
      return store.history.filter((i) => bulk.selectedIds.has(i.id));
    },
    get allSelectedFavorites() {
      return store.history.filter((i) => bulk.selectedIds.has(i.id)).every((i) => i.favorite);
    },
    captureAffected: (ids) => captureAffectedItems(store.current, ids, bulk.selectedIds),
    rollbackAffected: (snapshot) => {
      store.current = restoreAffectedItems(store.current, snapshot);
    },
    statusMessage: "",
    translate: (key) => key,
  });
  untrack(() =>
    Object.assign(out, {
      store,
      history,
      search,
      bulk,
      setQuery: (value: string) => {
        query = value;
      },
    }),
  );
</script>

<div>{store.history.length} / {store.indexed?.length ?? 0}</div>
