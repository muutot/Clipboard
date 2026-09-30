// Route-facing wrapper around the item store: it owns the *only* reactive
// declaration the loaded records live in, and projects the four views.
//
// Why a module and not inline runes in `+page.svelte`:
//
// - `$state.raw` + replacing the whole store keeps the plain `Map` out of
//   Svelte's deep proxy, so a wide `ClipboardItem` keeps its identity and never
//   reaches an `invoke` payload as a proxy. See `docs/PITFALLS.md`.
// - A `Map` is not deep-proxied by `$state` at all, so an in-place `set()`
//   would update nothing. Every mutator in `item-store.ts` therefore returns
//   the next store and the caller assigns it through `current`.
// - Owning the runes here makes that contract executable: `item-store-view.test.ts`
//   drives the same code path the route uses, which inline component runes
//   cannot be tested for (the repo has no component-mount test setup).

import type { ClipboardItem } from "$lib/types/clipboard";
import { getDetailItem, getIndexedItems, getItems, type ItemStore } from "./item-store";

export interface ItemStoreView {
  /** The store every mutator works from. Assign to advance the projections. */
  current: ItemStore;
  /** Loaded history page, in display order. */
  readonly history: ClipboardItem[];
  /** Active search page, or `null` while no search is displayed. */
  readonly indexed: ClipboardItem[] | null;
  /** Record shown in the detail pane, or `null`. */
  readonly detail: ClipboardItem | null;
}

export function createItemStoreView(initial: ItemStore): ItemStoreView {
  let store = $state.raw(initial);
  const history = $derived(getItems(store, "history"));
  const indexed = $derived(getIndexedItems(store));
  const detail = $derived(getDetailItem(store));
  return {
    get current(): ItemStore {
      return store;
    },
    set current(next: ItemStore) {
      store = next;
    },
    // Getters, not copied values: `{ history }` would freeze the derived's
    // value at return time and every later read would be stale.
    get history(): ClipboardItem[] {
      return history;
    },
    get indexed(): ClipboardItem[] | null {
      return indexed;
    },
    get detail(): ClipboardItem | null {
      return detail;
    },
  };
}
