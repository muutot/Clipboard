import type { ClipboardItem, GeneralSettings } from "$lib/types/clipboard";
import type { ItemStoreView } from "$lib/utils/store/item-store-view.svelte";
import type { createHistoryController } from "./history.svelte";
import {
  applyItemPatches,
  removeItems,
  restoreAffectedItems,
  type AffectedItemSnapshot,
} from "$lib/utils/store/item-store";
import {
  hydrateClipboardItem,
  writeClipboardText,
  persistBatchFavorite,
  persistBatchRestore,
  persistBatchPermanentDelete,
  persistBatchDelete,
  persistHardDelete,
} from "$lib/services/clipboard";
import { showToast } from "$lib/services/toast";
import { planBulkCopy, planBulkDelete } from "$lib/utils/store/bulk-actions";
export interface BulkDependencies {
  itemStore: ItemStoreView;
  readonly settings: GeneralSettings;
  readonly selectedLoadedItems: ClipboardItem[];
  readonly allSelectedFavorites: boolean;
  history: ReturnType<typeof createHistoryController>;
  captureAffected(ids: ReadonlySet<string>): AffectedItemSnapshot;
  rollbackAffected(snapshot: AffectedItemSnapshot): void;
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

  function bulkFavorite() {
    const ids = [...selectedIds];
    const unfavorite = deps.allSelectedFavorites;
    const idSet = new Set(ids);

    // Snapshot before the optimistic patch: a whole-map restore would clobber
    // the pre-existing favorite flags of a mixed selection, while this funnel
    // restores only the affected rows captured at this moment.
    const snapshot = deps.captureAffected(idSet);
    const patch = new Map<string, Partial<ClipboardItem>>();
    for (const id of ids) patch.set(id, { favorite: !unfavorite });
    deps.itemStore.current = applyItemPatches(deps.itemStore.current, patch);

    void persistBatchFavorite(ids, !unfavorite)
      .then((updated) => {
        if (updated === false) throw new Error("batch favorite failed");
        showToast(
          unfavorite
            ? deps.translate("toast.bulkUnfavoriteSuccess", { count: ids.length })
            : deps.translate("toast.bulkFavoriteSuccess", { count: ids.length }),
          "success",
        );
        selectedIds = new Set();
      })
      .catch((error) => {
        console.error("Bulk favorite failed", error);
        deps.rollbackAffected(snapshot);
        deps.statusMessage = deps.translate("app.favoriteFailed");
        showToast(deps.translate("app.favoriteFailed"), "error");
      });
  }

  function bulkRestore() {
    const ids = deps.selectedLoadedItems.filter((item) => item.deleted).map((item) => item.id);
    if (ids.length === 0) return;

    const idSet = new Set(ids);
    const snapshot = deps.captureAffected(idSet);
    for (const id of ids) deps.history.addSuppressedId(id);
    deps.itemStore.current = applyItemPatches(
      deps.itemStore.current,
      new Map(ids.map((id) => [id, { deleted: false }])),
    );
    selectedIds = new Set([...selectedIds].filter((id) => !idSet.has(id)));

    void persistBatchRestore(ids)
      .then((restored) => {
        if (restored === false) throw new Error("batch restore failed");
        deps.history.invalidateActiveHistoryPagination();
        deps.history.invalidateDeletedHistoryPagination();
        showToast(deps.translate("toast.restoreSuccess", { count: ids.length }), "success");
      })
      .catch((error) => {
        console.error("Bulk restore failed", error);
        for (const id of ids) deps.history.deletedHistorySuppressedIds.delete(id);
        deps.rollbackAffected(snapshot);
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
        for (const id of ids) deps.history.deletedHistorySuppressedIds.delete(id);
        deps.rollbackAffected(snapshot);
        deps.statusMessage = deps.translate("app.deleteFailed");
        showToast(deps.translate("app.deleteFailed"), "error");
      });
  }

  function bulkDelete() {
    const selectedItems = deps.selectedLoadedItems;
    if (selectedItems.length === 0) return;

    const useRecycleBin = deps.settings.useRecycleBin;
    const { softIds, permanentIds, hardIds } = planBulkDelete(selectedItems, useRecycleBin);
    const operationIds = new Set([...softIds, ...permanentIds, ...hardIds]);
    if (operationIds.size === 0) return;

    const snapshot = deps.captureAffected(operationIds);

    for (const id of softIds) deps.history.deletedHistorySuppressedIds.delete(id);
    for (const id of permanentIds) deps.history.addSuppressedId(id);

    const removedOptimistic = new Set([...permanentIds, ...hardIds]);
    // Removed rows leave every view at once; soft rows are flagged in place, so
    // the open detail pane follows along without a write of its own.
    deps.itemStore.current = removeItems(deps.itemStore.current, removedOptimistic);
    deps.itemStore.current = applyItemPatches(
      deps.itemStore.current,
      new Map(softIds.map((id) => [id, { deleted: true }])),
    );
    selectedIds = new Set();

    const operations: {
      ids: string[];
      mode: "soft" | "permanent" | "hard";
      run: () => Promise<boolean | null>;
    }[] = [];
    if (softIds.length > 0) {
      operations.push({ ids: softIds, mode: "soft", run: () => persistBatchDelete(softIds) });
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
    ).then((outcomes) => {
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

      for (const id of successfulPermanent) deps.history.addSuppressedId(id);
      for (const id of permanentIds) {
        if (!successfulPermanent.has(id)) deps.history.deletedHistorySuppressedIds.delete(id);
      }
      for (const id of softIds) deps.history.deletedHistorySuppressedIds.delete(id);

      // Mirror exactly which backend transaction succeeded: return the ids this
      // batch touched to their pre-mutation state, then re-apply the per-outcome
      // transitions. Going through the funnel (rather than rebuilding whole
      // arrays from a snapshot) keeps records that arrived via clipboard events
      // during the async window — a whole-array rebuild dropped them entirely.
      let next = restoreAffectedItems(deps.itemStore.current, snapshot);
      next = removeItems(next, removedIds);
      next = applyItemPatches(
        next,
        new Map([...successfulSoft].map((id) => [id, { deleted: true }])),
      );
      deps.itemStore.current = next;
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
    });
  }
  return {
    bulkCopy,
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
