import type { ClipboardItem } from "$lib/types/clipboard";
import { applyItemPatches, type ItemStore } from "./item-store";
import type { ItemStoreView } from "./item-store-view.svelte";

type Field = "favorite" | "tags" | "title" | "customTitle" | "deleted";
type Value = ClipboardItem[Field];
export type ItemMetadataPatch = Partial<Pick<ClipboardItem, Exclude<Field, "deleted">>>;
export type ItemMutationPatch = Partial<Pick<ClipboardItem, Field>>;
const FIELDS: readonly Field[] = ["favorite", "tags", "title", "customTitle", "deleted"];
function fieldValue(item: ClipboardItem | undefined, field: Field): Value {
  return field === "deleted" ? !!item?.deleted : item?.[field];
}
interface PendingField {
  confirmed: Value;
  epoch: number;
  pending: Map<number, { value: Value; sent: boolean; superseded: boolean }>;
}

/** Ordered field writes project pending edits over the latest confirmed metadata/membership. */
export function createItemMutations(view: () => ItemStoreView) {
  const fields = new Map<string, Map<Field, PendingField>>();
  const tails = new Map<string, Promise<void>>();
  let revision = 0;
  let publishing = false;
  let alive = true;

  function project(store: ItemStore): ItemStore {
    const patches = new Map<string, ItemMutationPatch>();
    for (const [id, row] of fields) {
      if (!store.byId.has(id)) {
        fields.delete(id);
        continue;
      }
      const patch: Record<string, Value> = {};
      for (const [field, state] of row) {
        let value = state.confirmed;
        for (const edit of state.pending.values()) if (!edit.superseded) value = edit.value;
        if (!Object.is(fieldValue(store.byId.get(id), field), value)) patch[field] = value;
      }
      if (Object.keys(patch).length) patches.set(id, patch);
    }
    return applyItemPatches(store, patches);
  }

  function confirm(state: PendingField, value: Value) {
    state.confirmed = value;
    state.epoch++;
    // A canonical update owns the fields of requests already sent. Unsent
    // edits remain projected because they represent subsequent local intent.
    for (const edit of state.pending.values()) if (edit.sent) edit.superseded = true;
  }

  function publish(store = view().current) {
    if (!alive) return;
    publishing = true;
    try {
      view().current = project(store);
    } finally {
      publishing = false;
    }
  }

  /** Reconcile canonical partial writes without treating unrelated fields as acknowledgements. */
  function observe(previous: ItemStore, next: ItemStore): ItemStore {
    if (!alive || publishing) return next;
    for (const [id, row] of fields) {
      const current = next.byId.get(id);
      if (!current) {
        fields.delete(id);
        continue;
      }
      for (const [field, state] of row) {
        if (!Object.is(fieldValue(previous.byId.get(id), field), fieldValue(current, field))) {
          confirm(state, fieldValue(current, field));
        }
      }
    }
    return project(next);
  }

  /** Full backend records acknowledge even same-valued fields; call before folding the records. */
  function acceptRecords(records: readonly ClipboardItem[], deleted?: boolean) {
    if (!alive) return;
    for (const record of records) {
      for (const [field, state] of fields.get(record.id) ?? []) {
        // Persisted content DTOs omit receiver-owned membership. Native list
        // pages explicitly identify their active/bin query; content events do not.
        if (field === "deleted") {
          if (deleted !== undefined) confirm(state, deleted);
        } else confirm(state, record[field]);
      }
    }
  }

  /** Membership IDs acknowledge same-valued flags without inferring them from content DTOs. */
  function acceptMembership(deletedIds: readonly string[], restoredIds: readonly string[]) {
    if (!alive) return;
    for (const [ids, value] of [
      [deletedIds, true],
      [restoredIds, false],
    ] as const) {
      for (const id of ids) {
        const state = fields.get(id)?.get("deleted");
        if (state) confirm(state, value);
      }
    }
  }

  /** Event transforms must start from confirmed fields, never from a pending tag draft. */
  function confirmedItem(item: ClipboardItem): ClipboardItem {
    const row = fields.get(item.id);
    if (!row?.size) return item;
    return {
      ...item,
      ...Object.fromEntries([...row].map(([field, state]) => [field, state.confirmed])),
    };
  }

  function run<T>(
    patches: ReadonlyMap<string, ItemMutationPatch>,
    persist: () => Promise<T>,
  ): Promise<T> {
    if (!alive) return Promise.reject(new Error("item view is disposed"));
    const operation = ++revision;
    const owned = new Map<string, Map<Field, PendingField>>();
    for (const [id, patch] of patches) {
      const current = view().current.byId.get(id);
      if (!current) continue;
      const row = fields.get(id) ?? new Map<Field, PendingField>();
      const changed = new Map<Field, PendingField>();
      for (const field of FIELDS) {
        if (!Object.hasOwn(patch, field)) continue;
        const state = row.get(field) ?? {
          confirmed: fieldValue(current, field),
          epoch: 0,
          pending: new Map(),
        };
        state.pending.set(operation, { value: patch[field], sent: false, superseded: false });
        row.set(field, state);
        changed.set(field, state);
      }
      fields.set(id, row);
      owned.set(id, changed);
    }
    publish();

    // Dependencies always point to earlier operations. Disjoint rows can run
    // concurrently; a batch waits for all rows it will mutate, without deadlocks.
    const ids = [...patches.keys()];
    const predecessors = ids.flatMap((id) => (tails.has(id) ? [tails.get(id)!] : []));
    const write = Promise.all(predecessors).then(async () => {
      if (!alive) throw new Error("item view is disposed");
      const epochs = new Map<PendingField, number>();
      for (const row of owned.values()) {
        for (const state of row.values()) {
          epochs.set(state, state.epoch);
          state.pending.get(operation)!.sent = true;
        }
      }
      let succeeded = false;
      try {
        const result = await persist();
        if (result === false) throw new Error("record was not updated");
        succeeded = true;
        return result;
      } finally {
        if (alive) {
          for (const [id, row] of owned) {
            for (const [field, state] of row) {
              if (fields.get(id)?.get(field) !== state) continue;
              if (succeeded && state.epoch === epochs.get(state)) {
                state.confirmed = state.pending.get(operation)!.value;
              }
              state.pending.delete(operation);
            }
          }
          // Publish final confirmed values before releasing empty field states.
          publish();
          for (const [id, row] of fields) {
            for (const [field, state] of row) if (!state.pending.size) row.delete(field);
            if (!row.size) fields.delete(id);
          }
        }
      }
    });
    const tail = write.then(
      () => {},
      () => {},
    );
    for (const id of ids) tails.set(id, tail);
    void tail.then(() => {
      for (const id of ids) if (tails.get(id) === tail) tails.delete(id);
    });
    return write;
  }

  return {
    run,
    observe,
    acceptRecords,
    acceptMembership,
    confirmedItem,
    destroy() {
      alive = false;
      fields.clear();
      tails.clear();
    },
  };
}
