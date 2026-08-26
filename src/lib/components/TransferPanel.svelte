<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import Checkbox from "$lib/components/Checkbox.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import DatePicker from "$lib/components/DatePicker.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { formatBytes } from "$lib/utils/format";
  import {
    exportToFile,
    getExportFormats,
    getImportFormats,
    importFromFile,
    type ExportFormatInfo,
    type ImportFormatInfo,
  } from "$lib/services/storage";
  import { endOfDay, startOfDay } from "$lib/utils/date-query";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    maxItemCount: number;
    onfeedback: (message: string, success: boolean) => void;
    onadjustlimit: () => void;
  }

  let { maxItemCount, onfeedback, onadjustlimit }: Props = $props();

  const storageKinds: readonly {
    kind: "text" | "link" | "image" | "file";
    labelKey: "filter.text" | "filter.link" | "filter.image" | "filter.file";
  }[] = [
    { kind: "text", labelKey: "filter.text" },
    { kind: "link", labelKey: "filter.link" },
    { kind: "image", labelKey: "filter.image" },
    { kind: "file", labelKey: "filter.file" },
  ];

  let exportFormats = $state<ExportFormatInfo[]>([]);
  let exportFormat = $state("json");
  let importFormats = $state<ImportFormatInfo[]>([]);
  let importFormat = $state("pastebackup");
  let exportIncludeFavorites = $state(true);
  let exportContentTypes = $state<Set<string>>(new Set(["text", "link", "image", "file"]));
  let exportDateFrom = $state("");
  let exportDateTo = $state("");
  let exporting = $state(false);
  let importing = $state(false);
  let showLimitWarning = $state(false);
  let importTruncationCount = $state(0);

  $effect(() => {
    void loadExportFormats();
    void loadImportFormats();
  });

  function toggleExportContentType(kind: string) {
    const next = new Set(exportContentTypes);
    if (next.has(kind)) {
      next.delete(kind);
    } else {
      next.add(kind);
    }
    exportContentTypes = next;
  }

  function exportDateToMs(value: string, end: boolean): number | null {
    if (!value) return null;
    const timestamp = new Date(`${value}T00:00:00`).getTime();
    if (Number.isNaN(timestamp)) return null;
    return end ? endOfDay(timestamp) : startOfDay(timestamp);
  }

  async function loadExportFormats(): Promise<void> {
    try {
      const formats = await getExportFormats();
      exportFormats = formats;
      if (formats.length > 0 && !formats.some((format) => format.id === exportFormat)) {
        exportFormat = formats[0].id;
      }
    } catch (error) {
      console.error("Unable to load export formats", error);
      exportFormats = [];
    }
  }

  async function loadImportFormats(): Promise<void> {
    try {
      const formats = await getImportFormats();
      importFormats = formats;
      if (formats.length > 0 && !formats.some((format) => format.id === importFormat)) {
        importFormat = formats[0].id;
      }
    } catch (error) {
      console.error("Unable to load import formats", error);
      importFormats = [];
    }
  }

  async function handleExport() {
    if (!isTauriRuntime() || exporting || importing) return;
    const format = exportFormats.find((entry) => entry.id === exportFormat);
    if (!format) return;
    exporting = true;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const filePath = await save({
        defaultPath: `clipboard-export${format.extension}`,
        filters: [{ name: format.label, extensions: [format.extension.slice(1)] }],
      });
      if (!filePath) return;
      const result = await exportToFile(filePath, format.id, {
        includeFavorites: exportIncludeFavorites,
        dateFromMs: exportDateToMs(exportDateFrom, false),
        dateToMs: exportDateToMs(exportDateTo, true),
        contentTypes: Array.from(exportContentTypes),
      });
      onfeedback(
        _t("storage.exportSuccess", {
          path: result.path,
          size: formatBytes(result.byteCount),
        }),
        true,
      );
    } catch (error) {
      onfeedback(
        _t("storage.exportFailed", {
          error: error instanceof Error ? error.message : String(error),
        }),
        false,
      );
    } finally {
      exporting = false;
    }
  }

  async function handleImport() {
    if (!isTauriRuntime() || exporting || importing) return;
    const format = importFormats.find((entry) => entry.id === importFormat);
    if (!format) return;
    importing = true;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const filePath = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: format.label,
            extensions: [format.extension.slice(1)],
          },
        ],
      });
      if (!filePath) return;
      const result = await importFromFile(filePath);
      if (result.errors.length > 0) {
        const detail = `${_t("storage.importErrorsN", {
          count: result.errors.length,
        })} ${result.errors[0]}`;
        onfeedback(
          _t("storage.importPartial", {
            imported: result.importedCount,
            skipped: result.skippedCount,
            error: detail,
          }),
          false,
        );
      } else {
        onfeedback(
          _t("storage.importSuccess", {
            imported: result.importedCount,
            skipped: result.skippedCount,
          }),
          true,
        );
      }
      if (result.pendingTruncation > 0) {
        showLimitWarning = true;
        importTruncationCount = result.pendingTruncation;
      }
    } catch (error) {
      onfeedback(
        _t("storage.importFailed", {
          error: error instanceof Error ? error.message : String(error),
        }),
        false,
      );
    } finally {
      importing = false;
    }
  }
</script>

<div class="settings-scroll">
  <section class="setting-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="download" size={17} /></span>
      <div>
        <strong>{_t("storage.transferTitle")}</strong>
        <p>{_t("storage.transferDesc")}</p>
      </div>
    </div>
    <div class="transfer-actions">
      <div class="transfer-group">
        <span class="transfer-label">{_t("storage.exportLabel")}</span>
        <CustomSelect
          value={exportFormat}
          disabled={exporting || importing || exportFormats.length === 0}
          ariaLabel={_t("storage.exportLabel")}
          options={exportFormats.map((format) => ({
            value: format.id,
            label: format.label,
          }))}
          onchange={(v) => (exportFormat = v as string)}
        />
        <button
          type="button"
          class="settings-action-btn"
          disabled={exporting || importing || exportFormats.length === 0}
          onclick={handleExport}
        >
          {exporting ? _t("storage.exporting") : _t("storage.exportAction")}
        </button>
      </div>
      <div class="transfer-group">
        <span class="transfer-label">{_t("storage.importLabel")}</span>
        <CustomSelect
          value={importFormat}
          disabled={exporting || importing || importFormats.length === 0}
          ariaLabel={_t("storage.importLabel")}
          options={importFormats.map((format) => ({
            value: format.id,
            label: format.label,
          }))}
          onchange={(v) => (importFormat = v as string)}
        />
        <button
          type="button"
          class="settings-action-btn"
          disabled={exporting || importing || importFormats.length === 0}
          onclick={handleImport}
        >
          {importing ? _t("storage.importing") : _t("storage.importAction")}
        </button>
      </div>
    </div>
    {#if showLimitWarning}
      <div class="transfer-limit-warning">
        <span
          >{_t("storage.importTruncationWarning", {
            max: maxItemCount,
            count: importTruncationCount,
          })}</span
        >
        <button type="button" class="settings-action-btn" onclick={onadjustlimit}>
          {_t("storage.importAdjustLimit")}
        </button>
      </div>
    {/if}
    <div class="export-options">
      <div class="export-option-row">
        <span class="export-option-label">{_t("storage.exportFavorites")}</span>
        <label class="export-check">
          <Checkbox
            checked={exportIncludeFavorites}
            onchange={(checked) => (exportIncludeFavorites = checked)}
            size={15}
          />
          <span>{_t("storage.exportIncludeFavorites")}</span>
        </label>
      </div>
      <div class="export-option-row">
        <span class="export-option-label">{_t("storage.exportContentTypes")}</span>
        <div class="export-kind-checks">
          {#each storageKinds as kindInfo (kindInfo.kind)}
            <label class="export-check">
              <Checkbox
                checked={exportContentTypes.has(kindInfo.kind)}
                onchange={() => toggleExportContentType(kindInfo.kind)}
                size={15}
              />
              <span>{_t(kindInfo.labelKey)}</span>
            </label>
          {/each}
        </div>
      </div>
      <div class="export-option-row export-date-row">
        <span class="export-option-label">{_t("storage.exportDateRange")}</span>
        <DatePicker
          value={exportDateFrom}
          onchange={(v) => (exportDateFrom = v)}
          ariaLabel={_t("storage.exportDateFrom")}
        />
        <span class="export-date-separator">–</span>
        <DatePicker
          value={exportDateTo}
          onchange={(v) => (exportDateTo = v)}
          ariaLabel={_t("storage.exportDateTo")}
        />
      </div>
    </div>
  </section>
</div>
