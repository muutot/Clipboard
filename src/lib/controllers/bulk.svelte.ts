import type { ClipboardItem, GeneralSettings } from "#lib/types/clipboard.ts";
import type { ItemStoreView } from "#lib/utils/store/item-store-view.svelte.ts";
import type { createItemMutations } from "#lib/utils/store/item-mutations.ts";
import type { createHistoryController } from "./history.svelte";
import {
  removeItems,
  restoreAffectedItems,
  type AffectedItemOptions,
  type AffectedItemSnapshot,
} from "#lib/utils/store/item-store.ts";
import {
  hydrateClipboardItem,
  pasteBulkPayload,
  persistLastUsed,
  beginPasteBatch,
  endPasteBatch,
  writeClipboardText,
  persistBatchFavorite,
  persistBatchRestore,
  persistBatchPermanentDelete,
  persistBatchDelete,
  persistHardDelete,
} from "#lib/services/clipboard.ts";
import { showToast } from "#lib/services/toast.ts";
import { planBulkCopy, planBulkDelete, planBulkPaste } from "#lib/utils/store/bulk-actions.ts";
export interface BulkDependencies {
  itemStore: ItemStoreView;
  metadataMutations: ReturnType<typeof createItemMutations>;
  readonly settings: GeneralSettings;
  readonly selectedLoadedItems: ClipboardItem[];
  readonly allSelectedFavorites: boolean;
  history: ReturnType<typeof createHistoryController>;
  captureAffected(ids: ReadonlySet<string>, options?: AffectedItemOptions): AffectedItemSnapshot;
  rollbackAffected(snapshot: AffectedItemSnapshot): void;
  finishAffected(snapshot: AffectedItemSnapshot): void;
  statusMessage: string;
  translate(key: string, params?: Record<string, string | number>): string;
}
/** Owns selection and optimistic bulk outcomes; updates only the shared item store. */
export function createBulkController(deps: BulkDependencies) {
  let selectedIds = $state<Set<string>>(new Set());

  // --- Bulk operations ---

  async function bulkCopy() {
    let selectedItems: ClipboardItem[];
    try {
      // Sequential reads bound transient IPC/body memory while preserving selection order.
      selectedItems = [];
      for (const item of deps.selectedLoadedItems)
        selectedItems.push(await hydrateClipboardItem(item));
    } catch (error) {
      console.error("Unable to load selection for copy", error);
      showToast(deps.translate("toast.copyFailed"), "error");
      return;
    }
    // Text/link rows carry the full content in `textContent` while `title`
    // is only the first line; image/file rows contribute their source path
    // (see `planBulkCopy`), never the display title alone.
    const { text, copiedCount } = planBulkCopy(selectedItems);
    if (copiedCount === 0) {
      showToast(deps.translate("toast.copyFailed"), "error");
      return;
    }
    void writeClipboardText(text)
      .then(() => {
        showToast(deps.translate("toast.bulkCopySuccess", { count: copiedCount }), "success");
      })
      .catch(() => {
        showToast(deps.translate("toast.copyFailed"), "error");
      });
  }

  // Bulk paste: merge the whole selection into ONE clipboard payload and
  // deliver it with ONE native paste, in selection order. The previous
  // per-item loop raced the target's asynchronous clipboard read — the next
  // staging landed 23–30 ms after the previous Ctrl+V injection, so the
  // target sometimes read item N+1's content for item N. A single write
  // cannot race itself, which removes the bug class without adding delays.
  let pasting = false;
  async function bulkPasteAll() {
    // A second click while a paste run is in flight would stage two payloads
    // over each other at the paste target; ignore it until the run ends.
    if (pasting) return;
    const items = [...deps.selectedLoadedItems];
    if (items.length === 0) return;
    pasting = true;
    let batchToken: string | null = null;
    try {
      // Hydrate before reserving the lease: a slow content read must not
      // expire the batch, and a hydrate failure must leave the clipboard
      // untouched.
      let hydrated: ClipboardItem[];
      try {
        hydrated = [];
        for (const item of items) hydrated.push(await hydrateClipboardItem(item));
      } catch (error) {
        console.error("Unable to load selection for paste", error);
        showToast(deps.translate("toast.pasteFailed"), "error");
        return;
      }
      const plan = planBulkPaste(hydrated);
      if (!plan) {
        showToast(deps.translate("toast.pasteFailed"), "error");
        return;
      }
      batchToken = await beginPasteBatch();
      if (!batchToken) {
        showToast(deps.translate("toast.pasteFailed"), "error");
        return;
      }
      const outcome = await pasteBulkPayload(plan.payload, { batchToken });
      if (outcome === "failed") {
        // Nothing was delivered: keep every row selected for a retry.
        showToast(deps.translate("toast.pasteFailed"), "error");
        return;
      }
      // Delivery succeeded (browser preview stages the same single payload).
      // Usage metadata is best-effort, mirroring the per-item path's
      // recordSuccessfulUsage; a persist failure never undoes the paste.
      const delivered = new Set(plan.ids);
      for (const id of plan.ids) {
        try {
          await persistLastUsed(id);
        } catch (error) {
          console.error("Unable to persist last-used item", error);
        }
      }
      // User selection can change while IPC is pending. Remove only the rows
      // this run delivered, retaining newly selected other rows.
      selectedIds = new Set([...selectedIds].filter((id) => !delivered.has(id)));
      showToast(deps.translate("toast.bulkPasteSuccess", { count: plan.ids.length }), "success");
    } catch (error) {
      console.error("Unable to start bulk paste", error);
      showToast(deps.translate("toast.pasteFailed"), "error");
    } finally {
      if (batchToken) {
        try {
          await endPasteBatch(batchToken);
        } catch (error) {
          console.error("Unable to release bulk paste target", error);
        }
      }
      pasting = false;
    }
  }

  function bulkFavorite() {
    const ids = [...selectedIds];
    const unfavorite = deps.allSelectedFavorites;
    const idSet = new Set(ids);

    const patch = new Map<string, Partial<ClipboardItem>>();
    for (const id of ids) patch.set(id, { favorite: !unfavorite });
    void deps.metadataMutations
      .run(patch, () => persistBatchFavorite(ids, !unfavorite))
      .then(() => {
        showToast(
          unfavorite
            ? deps.translate("toast.bulkUnfavoriteSuccess", { count: ids.length })
            : deps.translate("toast.bulkFavoriteSuccess", { count: ids.length }),
          "success",
        );
        selectedIds = new Set([...selectedIds].filter((id) => !idSet.has(id)));
      })
      .catch((error) => {
        console.error("Bulk favorite failed", error);
        deps.statusMessage = deps.translate("app.favoriteFailed");
        showToast(deps.translate("app.favoriteFailed"), "error");
      });
  }

  function bulkRestore() {
    const ids = deps.selectedLoadedItems.filter((item) => item.deleted).map((item) => item.id);
    if (ids.length === 0) return;

    const idSet = new Set(ids);
    const patch = new Map<string, Partial<ClipboardItem>>(
      ids.map((id) => [id, { deleted: false }]),
    );
    for (const id of ids) deps.history.addSuppressedId(id);
    selectedIds = new Set([...selectedIds].filter((id) => !idSet.has(id)));

    void deps.metadataMutations
      .run(patch, () => persistBatchRestore(ids))
      .then(() => {
        deps.history.invalidateActiveHistoryPagination();
        deps.history.invalidateDeletedHistoryPagination();
        showToast(deps.translate("toast.restoreSuccess", { count: ids.length }), "success");
      })
      .catch((error) => {
        console.error("Bulk restore failed", error);
        for (const id of ids) {
          if (deps.itemStore.current.byId.get(id)?.deleted) {
            deps.history.deletedHistorySuppressedIds.delete(id);
            selectedIds = new Set([...selectedIds, id]);
          }
        }
        deps.statusMessage = deps.translate("app.deleteFailed");
        showToast(deps.translate("app.deleteFailed"), "error");
      });
  }

  function bulkPermanentDelete() {
    const ids = deps.selectedLoadedItems.filter((item) => item.deleted).map((item) => item.id);
    if (ids.length === 0) return;

    const idSet = new Set(ids);
    const snapshot = deps.captureAffected(idSet);
    for (const id of ids) deps.history.addSuppressedId(id);
    deps.itemStore.current = removeItems(deps.itemStore.current, idSet);
    selectedIds = new Set([...selectedIds].filter((id) => !idSet.has(id)));

    void persistBatchPermanentDelete(ids)
      .then((removed) => {
        if (removed === false) throw new Error("batch permanent delete failed");
        deps.history.invalidateDeletedHistoryPagination();
        showToast(deps.translate("toast.bulkDeleteSuccess", { count: ids.length }), "success");
      })
      .catch((error) => {
        console.error("Bulk permanent delete failed", error);
        for (const id of ids)
          if (!snapshot.invalidatedIds?.has(id))
            deps.history.deletedHistorySuppressedIds.delete(id);
        deps.rollbackAffected(snapshot);
        deps.statusMessage = deps.translate("app.deleteFailed");
        showToast(deps.translate("app.deleteFailed"), "error");
      })
      .finally(() => deps.finishAffected(snapshot));
  }

  function bulkDelete() {
    const selectedItems = deps.selectedLoadedItems;
    if (selectedItems.length === 0) return;

    const useRecycleBin = deps.settings.useRecycleBin;
    const { softIds, permanentIds, hardIds } = planBulkDelete(selectedItems, useRecycleBin);
    const operationIds = new Set([...softIds, ...permanentIds, ...hardIds]);
    if (operationIds.size === 0) return;

    const softPatches = new Map<string, Partial<ClipboardItem>>(
      softIds.map((id) => [id, { deleted: true }]),
    );
    const snapshot = deps.captureAffected(new Set([...permanentIds, ...hardIds]));

    for (const id of softIds) deps.history.deletedHistorySuppressedIds.delete(id);
    for (const id of permanentIds) deps.history.addSuppressedId(id);

    const removedOptimistic = new Set([...permanentIds, ...hardIds]);
    // Removed rows leave every view at once; soft rows are flagged in place, so
    // the open detail pane follows along without a write of its own.
    deps.itemStore.current = removeItems(deps.itemStore.current, removedOptimistic);
    selectedIds = new Set();

    const operations: {
      ids: string[];
      mode: "soft" | "permanent" | "hard";
      run: () => Promise<boolean | null>;
    }[] = [];
    if (softIds.length > 0) {
      operations.push({
        ids: softIds,
        mode: "soft",
        run: () => deps.metadataMutations.run(softPatches, () => persistBatchDelete(softIds)),
      });
    }
    if (permanentIds.length > 0) {
      operations.push({
        ids: permanentIds,
        mode: "permanent",
        run: () => persistBatchPermanentDelete(permanentIds),
      });
    }
    for (const id of hardIds) {
      operations.push({ ids: [id], mode: "hard", run: () => persistHardDelete(id) });
    }

    void Promise.all(
      operations.map(async (operation) => {
        try {
          const result = await operation.run();
          return { ...operation, ok: result !== false };
        } catch (error) {
          console.error(
            operation.mode === "permanent"
              ? "Bulk permanent delete failed"
              : operation.mode === "hard"
                ? "Bulk hard delete failed"
                : "Bulk delete failed",
            error,
          );
          return { ...operation, ok: false };
        }
      }),
    )
      .then((outcomes) => {
        const successfulSoft = new Set(
          outcomes.filter((outcome) => outcome.ok && outcome.mode === "soft").flatMap((o) => o.ids),
        );
        const successfulPermanent = new Set(
          outcomes
            .filter((outcome) => outcome.ok && outcome.mode === "permanent")
            .flatMap((o) => o.ids),
        );
        const successfulHard = new Set(
          outcomes.filter((outcome) => outcome.ok && outcome.mode === "hard").flatMap((o) => o.ids),
        );
        const failedIds = new Set(outcomes.filter((outcome) => !outcome.ok).flatMap((o) => o.ids));
        const removedIds = new Set([...successfulPermanent, ...successfulHard]);
        const succeededIds = new Set([...successfulSoft, ...removedIds]);

        for (const id of permanentIds) {
          if (!successfulPermanent.has(id) && !snapshot.invalidatedIds?.has(id))
            deps.history.deletedHistorySuppressedIds.delete(id);
        }

        // Success already has its optimistic transition and canonical broadcast.
        // Reapplying it here would undo a later restore or newly loaded record.
        // Only failed transactions own a rollback, including their detail slot.
        const failedSnapshot: AffectedItemSnapshot = {
          ...snapshot,
          previous: new Map([...snapshot.previous].filter(([id]) => failedIds.has(id))),
          patches: new Map([...snapshot.patches].filter(([id]) => failedIds.has(id))),
          detailId:
            snapshot.detailId !== null && failedIds.has(snapshot.detailId)
              ? snapshot.detailId
              : null,
        };
        deps.itemStore.current = restoreAffectedItems(deps.itemStore.current, failedSnapshot);
        selectedIds = new Set([...selectedIds].filter((id) => !succeededIds.has(id)));

        // Failed (and partially failed) batches skip the success-path
        // invalidations, so resync from the backend explicitly.
        if (successfulSoft.size > 0 || successfulPermanent.size > 0 || failedIds.size > 0) {
          deps.history.invalidateDeletedHistoryPagination();
        }
        if (successfulSoft.size > 0 || successfulHard.size > 0 || failedIds.size > 0) {
          deps.history.invalidateActiveHistoryPagination();
        }
        if (failedIds.size > 0) {
          deps.statusMessage = deps.translate("app.deleteFailed");
          showToast(deps.translate("app.deleteFailed"), "error");
        } else {
          showToast(
            deps.translate("toast.bulkDeleteSuccess", { count: succeededIds.size }),
            "success",
          );
        }
      })
      .finally(() => deps.finishAffected(snapshot));
  }
  return {
    bulkCopy,
    bulkPasteAll,
    bulkFavorite,
    bulkRestore,
    bulkPermanentDelete,
    bulkDelete,
    get selectedIds() {
      return selectedIds;
    },
    set selectedIds(value: Set<string>) {
      selectedIds = value;
    },
  };
}
