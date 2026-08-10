<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import SettingEntry from "$lib/components/SettingEntry.svelte";
  import { messages, resolvePath, locale } from "$lib/i18n";
  import type { Locale } from "$lib/i18n/types";
  import type {
    CardActionsDisplay,
    SearchSuggestionMode,
    SortRule,
    WindowConfig,
    WindowEffect,
  } from "$lib/types/clipboard";
  import { generalSettings, getWindowConfig, setWindowConfig } from "$lib/services/settings";
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { isTauriRuntime } from "$lib/services/runtime";
  import type { SettingEntryConfig } from "$lib/types/settings-entry";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  let _cachedWindowConfig: WindowConfig | null = null;

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
    section?: "search" | "items" | "window" | "general";
  }

  let { onclose, showHeader = true, section = "search" }: Props = $props();

  let s = $state($generalSettings);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let privacyPaused = $state(false);
  let privacyLoading = $state(true);
  let windowConfig = $state<WindowConfig | null>(
    _cachedWindowConfig ?? { launchAtStartup: false, closeToTray: true, singleInstance: true },
  );
  let windowConfigLoading = $state(!_cachedWindowConfig);
  let windowConfigSaving = $state(false);
  let sortDragIdx = $state<number | null>(null);
  let sortDragOverIdx = $state<number | null>(null);
  let sortListEl = $state<HTMLDivElement | null>(null);
  let feedbackTimer: ReturnType<typeof setTimeout> | undefined;
  let stopPointerDrag: (() => void) | undefined;

  const ALL_SORT_FIELDS: SortRule["field"][] = [
    "createdAt",
    "lastUsedAt",
    "title",
    "size",
    "kind",
    "favorite",
  ];

  const SORT_FIELD_LABELS: Record<SortRule["field"], string> = {
    createdAt: "general.sortFieldCreatedAt",
    lastUsedAt: "general.sortFieldLastUsedAt",
    title: "general.sortFieldTitle",
    size: "general.sortFieldSize",
    kind: "general.sortFieldKind",
    favorite: "general.sortFieldFavorite",
  };

  function pointerDragStart(idx: number, _e: PointerEvent) {
    stopPointerDrag?.();
    sortDragIdx = idx;
    sortDragOverIdx = null;
    const rows = sortListEl?.querySelectorAll<HTMLElement>(".sort-rule-row");

    function onMove(ev: PointerEvent) {
      if (!rows || rows.length === 0) return;
      let target: number | null = null;
      for (let i = 0; i < rows.length; i++) {
        const rect = rows[i].getBoundingClientRect();
        if (ev.clientY >= rect.top && ev.clientY <= rect.bottom) {
          target = i;
          break;
        }
      }
      for (let i = 0; i < rows.length; i++) {
        if (target !== null && i === target && i !== sortDragIdx) {
          rows[i].classList.add("sort-drag-over");
        } else {
          rows[i].classList.remove("sort-drag-over");
        }
      }
    }

    function cleanup() {
      rows?.forEach((row) => row.classList.remove("sort-drag-over"));
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
      if (stopPointerDrag === cleanup) stopPointerDrag = undefined;
    }

    function onUp() {
      const target = [...(rows ?? [])].findIndex((r) => r.classList.contains("sort-drag-over"));
      if (target !== -1 && target !== sortDragIdx) {
        moveSortRule(sortDragIdx!, target);
      }
      sortDragIdx = null;
      sortDragOverIdx = null;
      cleanup();
    }

    stopPointerDrag = cleanup;
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  }

  function moveSortRule(fromIdx: number, toIdx: number) {
    if (fromIdx === toIdx) return;
    const newRules = [...s.searchSortRules];
    const [removed] = newRules.splice(fromIdx, 1);
    newRules.splice(toIdx, 0, removed);
    generalSettings.updateSetting("searchSortRules", newRules);
  }

  function addSortRule() {
    const used = new Set(s.searchSortRules.map((r: SortRule) => r.field));
    const field = (ALL_SORT_FIELDS.find((f) => !used.has(f)) ?? "createdAt") as SortRule["field"];
    generalSettings.updateSetting("searchSortRules", [
      ...s.searchSortRules,
      { field, direction: "desc" },
    ]);
  }

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  $effect(() => {
    let cancelled = false;
    void getWindowConfig()
      .then((config) => {
        if (!cancelled) {
          _cachedWindowConfig = config;
          windowConfig = config;
        }
      })
      .catch(() => {
        if (!cancelled) showFeedback(_t("general.windowConfigLoadFailed"), false);
      })
      .finally(() => {
        if (!cancelled) windowConfigLoading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  function showFeedback(message: string, success: boolean) {
    feedback = message;
    feedbackSuccess = success;
    if (feedbackTimer !== undefined) clearTimeout(feedbackTimer);
    feedbackTimer = setTimeout(() => {
      feedbackTimer = undefined;
      feedback = "";
    }, 2000);
  }

  onDestroy(() => {
    stopPointerDrag?.();
    if (feedbackTimer !== undefined) clearTimeout(feedbackTimer);
  });

  function changeLanguage(lang: Locale) {
    generalSettings.updateSetting("language", lang);
    locale.set(lang);
    showFeedback(
      _t(lang === "zh-CN" ? "general.languageSwitchedZh" : "general.languageSwitchedEn"),
      true,
    );
  }

  onMount(() => {
    let disposed = false;
    let unlistenPrivacyPause: (() => void) | undefined;
    void loadPrivacyStatus();
    if (isTauriRuntime()) {
      listen<boolean>("privacy-pause-changed", (event) => {
        privacyPaused = event.payload;
      }).then((unlisten) => {
        if (disposed) unlisten();
        else unlistenPrivacyPause = unlisten;
      });
    }
    return () => {
      disposed = true;
      unlistenPrivacyPause?.();
    };
  });

  async function loadPrivacyStatus() {
    if (!isTauriRuntime()) {
      privacyLoading = false;
      return;
    }

    try {
      const status = await invoke<{ paused: boolean }>("get_privacy_status");
      privacyPaused = status.paused;
    } catch (error) {
      console.error("Unable to load privacy status", error);
    } finally {
      privacyLoading = false;
    }
  }

  async function togglePrivacyPause() {
    if (!isTauriRuntime() || privacyLoading) return;
    privacyLoading = true;

    try {
      privacyPaused = await invoke<boolean>("toggle_privacy_pause");
      showFeedback(_t(privacyPaused ? "capture.paused" : "capture.resumed"), true);
    } catch (error) {
      console.error("Unable to toggle privacy pause", error);
      showFeedback(error instanceof Error ? error.message : String(error), false);
    } finally {
      privacyLoading = false;
    }
  }

  async function changeWindowSetting(key: "launchAtStartup" | "closeToTray", value: boolean) {
    if (!windowConfig || windowConfigSaving) return;
    const previous = windowConfig;
    windowConfig = { ...previous, [key]: value };
    windowConfigSaving = true;
    try {
      await setWindowConfig({ [key]: value });
    } catch {
      windowConfig = previous;
      showFeedback(_t("general.windowConfigUpdateFailed"), false);
    } finally {
      windowConfigSaving = false;
    }
  }

  const searchEntries: SettingEntryConfig[] = $derived([
    {
      type: "select",
      icon: "search",
      label: _t("general.searchSuggestionMode"),
      desc: _t("general.searchSuggestionModeDescription"),
      get: () => s.searchSuggestionMode,
      options: [
        { value: "off", label: _t("general.searchSuggestionOff") },
        { value: "panel", label: _t("general.searchSuggestionPanel") },
        { value: "inline", label: _t("general.searchSuggestionInline") },
      ],
      set: (v) => generalSettings.updateSetting("searchSuggestionMode", v as SearchSuggestionMode),
    },
    {
      type: "toggle",
      icon: "clock",
      label: _t("general.searchHistory"),
      desc: _t("general.searchHistoryDescription"),
      get: () => s.searchHistoryEnabled,
      set: (v) => generalSettings.updateSetting("searchHistoryEnabled", v),
    },
    {
      type: "text",
      icon: "search",
      label: _t("general.searchPlaceholder"),
      desc: _t("general.searchPlaceholderDescription"),
      get: () => s.searchPlaceholder,
      set: (v) => generalSettings.updateSetting("searchPlaceholder", v),
      maxlength: 80,
      placeholder: _t("app.searchPlaceholder"),
      actionLabel: _t("general.searchPlaceholderDefault"),
      actionVisible: () => s.searchPlaceholder.trim().length > 0,
      onaction: () => generalSettings.updateSetting("searchPlaceholder", ""),
    },
    {
      type: "custom",
      variant: "column",
      id: "general.search-sort-rules",
      icon: "sliders",
      label: _t("general.searchSortRules"),
      desc: _t("general.searchSortRulesDescription"),
      actionLabel: () => `+ ${_t("general.sortAddRule")}`,
      actionVisible: () =>
        s.searchSortRules.length < 3 && s.searchSortRules.length < ALL_SORT_FIELDS.length,
      onaction: addSortRule,
    },
    {
      type: "slider",
      icon: "search",
      label: _t("general.searchPageSizeLimit"),
      desc: _t("general.searchPageSizeLimitDescription"),
      get: () => s.searchPageSizeLimit,
      set: (v) => generalSettings.updateSetting("searchPageSizeLimit", v),
      min: 50,
      max: 1000,
      step: 50,
      suffix: ` ${_t("general.searchPageSizeLimitUnit")}`,
    },
    {
      type: "slider",
      icon: "search",
      label: _t("general.searchPageSize"),
      desc: _t("general.searchPageSizeDescription"),
      get: () => Math.min(s.display.searchPageSize, s.searchPageSizeLimit),
      set: (v) =>
        generalSettings.updateSetting("display", {
          ...s.display,
          searchPageSize: Math.min(v, s.searchPageSizeLimit),
        }),
      min: 50,
      max: () => Math.min(s.searchPageSizeLimit, 500),
      step: 50,
      suffix: ` ${_t("general.searchPageSizeUnit")}`,
    },
    {
      type: "slider",
      icon: "search",
      label: _t("general.searchCacheSize"),
      desc: _t("general.searchCacheSizeDescription"),
      get: () => s.searchCacheSize,
      set: (v) => generalSettings.updateSetting("searchCacheSize", v),
      min: 200,
      max: 2000,
      step: 50,
      suffix: ` ${_t("general.searchCacheSizeUnit")}`,
    },
    {
      type: "select",
      icon: "sliders",
      label: _t("general.searchCacheEviction"),
      desc: _t("general.searchCacheEvictionDescription"),
      get: () => s.searchCacheEviction,
      options: [
        { value: "fifo", label: _t("general.searchCacheEvictionFifo") },
        { value: "lru", label: _t("general.searchCacheEvictionLru") },
      ],
      set: (v) => generalSettings.updateSetting("searchCacheEviction", v as "fifo" | "lru"),
    },
    {
      type: "select",
      icon: "search",
      label: _t("general.searchIndexSyncMode"),
      desc: _t("general.searchIndexSyncModeDescription"),
      get: () => s.searchIndexSyncMode,
      options: [
        { value: "lazy", label: _t("general.searchIndexSyncModeLazy") },
        { value: "background", label: _t("general.searchIndexSyncModeBackground") },
      ],
      set: (v) => generalSettings.updateSetting("searchIndexSyncMode", v as "lazy" | "background"),
    },
  ]);

  const itemsEntries: SettingEntryConfig[] = $derived([
    {
      type: "select",
      icon: "grid",
      label: _t("general.cardActionsDisplay"),
      desc: _t("general.cardActionsDisplayDescription"),
      get: () => s.cardActionsDisplay,
      options: [
        { value: "hover", label: _t("general.cardActionsHover") },
        { value: "always", label: _t("general.cardActionsAlways") },
      ],
      set: (v) => generalSettings.updateSetting("cardActionsDisplay", v as CardActionsDisplay),
    },
    {
      type: "toggle",
      icon: "copy",
      label: _t("general.quickCopyBadge"),
      desc: _t("general.quickCopyBadgeDescription"),
      get: () => s.quickCopyBadgeAlwaysVisible,
      set: (v) => generalSettings.updateSetting("quickCopyBadgeAlwaysVisible", v),
    },
    {
      type: "toggle",
      icon: "grid",
      label: _t("general.pinCopiedToTop"),
      desc: _t("general.pinCopiedToTopDescription"),
      get: () => s.pinCopiedToTop,
      set: (v) => generalSettings.updateSetting("pinCopiedToTop", v),
    },
    {
      type: "toggle",
      icon: "scan",
      label: _t("general.pasteCleaning"),
      desc: _t("general.pasteCleaningDescription"),
      get: () => s.pasteCleaningEnabled,
      set: (v) => generalSettings.updateSetting("pasteCleaningEnabled", v),
    },
    {
      type: "toggle",
      icon: "clipboard",
      label: _t("general.doubleClickPaste"),
      desc: _t("general.doubleClickPasteDescription"),
      get: () => s.doubleClickPaste,
      set: (v) => generalSettings.updateSetting("doubleClickPaste", v),
    },
    {
      type: "slider",
      icon: "file",
      label: _t("general.pageSize"),
      desc: _t("general.pageSizeDescription"),
      get: () => Math.min(s.display.pageSize, s.pageSizeLimit),
      set: (v) =>
        generalSettings.updateSetting("display", {
          ...s.display,
          pageSize: Math.min(v, s.pageSizeLimit),
        }),
      min: 50,
      max: () => Math.min(s.pageSizeLimit, 300),
      step: 50,
      suffix: ` ${_t("general.pageSizeUnit")}`,
    },
    {
      type: "slider",
      icon: "file",
      label: _t("general.pageSizeLimit"),
      desc: _t("general.pageSizeLimitDescription"),
      get: () => s.pageSizeLimit,
      set: (v) => {
        generalSettings.updateSetting("pageSizeLimit", v);
        if (s.display.pageSize > v) {
          generalSettings.updateSetting("display", { ...s.display, pageSize: v });
        }
      },
      min: 500,
      max: 6000,
      step: 100,
      suffix: ` ${_t("general.pageSizeLimitUnit")}`,
    },
    {
      type: "slider",
      icon: "file",
      label: _t("general.loadTolerance"),
      desc: _t("general.loadToleranceDescription"),
      get: () => s.loadTolerance,
      set: (v) => generalSettings.updateSetting("loadTolerance", v),
      min: 50,
      max: 500,
      step: 50,
      suffix: ` ${_t("general.loadToleranceUnit")}`,
    },
    {
      type: "toggle",
      icon: "eye",
      label: _t("general.showSecondaryText"),
      desc: _t("general.showSecondaryTextDescription"),
      get: () => s.display.showSecondaryText,
      set: (v) => generalSettings.updateSetting("display", { ...s.display, showSecondaryText: v }),
    },
    {
      type: "slider",
      icon: "text",
      label: _t("general.maxTextLines"),
      desc: _t("general.maxTextLinesDescription"),
      get: () => s.display.maxTextLines,
      set: (v) => generalSettings.updateSetting("display", { ...s.display, maxTextLines: v }),
      min: 1,
      max: 12,
      suffix: ` ${_t("general.maxTextLinesUnit")}`,
    },
  ]);

  const generalEntries: SettingEntryConfig[] = $derived([
    {
      type: "custom",
      variant: "toggle",
      id: "general.language",
      icon: "globe",
      label: _t("general.language"),
      desc: _t("general.languageDescription"),
    },
    {
      type: "toggle",
      icon: "clock",
      label: _t("general.launchAtStartup"),
      desc: _t("general.launchAtStartupDescription"),
      get: () => windowConfig?.launchAtStartup ?? false,
      set: (v) => void changeWindowSetting("launchAtStartup", v),
      disabled: () => windowConfigLoading || windowConfigSaving || !windowConfig,
    },
    {
      type: "custom",
      variant: "toggle",
      id: "recording.pause",
      icon: "pause",
      label: _t("capture.pauseTitle"),
      desc: _t("capture.pauseDescription"),
    },
    {
      type: "toggle",
      icon: "trash",
      label: _t("general.useRecycleBin"),
      desc: _t("general.useRecycleBinDescription"),
      get: () => s.useRecycleBin,
      set: (v) => generalSettings.updateSetting("useRecycleBin", v),
    },
    {
      type: "toggle",
      icon: "info",
      label: _t("general.toastNotifications"),
      desc: _t("general.toastNotificationsDescription"),
      get: () => s.showToastNotifications,
      set: (v) => generalSettings.updateSetting("showToastNotifications", v),
    },
    {
      type: "toggle",
      icon: "clipboard",
      label: _t("general.closeToTray"),
      desc: _t("general.closeToTrayDescription"),
      get: () => windowConfig?.closeToTray ?? false,
      set: (v) => void changeWindowSetting("closeToTray", v),
      disabled: () => windowConfigLoading || windowConfigSaving || !windowConfig,
    },
    {
      type: "toggle",
      icon: "copy",
      label: _t("general.useSystemTitleBar"),
      desc: _t("general.useSystemTitleBarDescription"),
      get: () => s.useSystemTitleBar,
      set: (v) => generalSettings.updateSetting("useSystemTitleBar", v),
    },
    {
      type: "toggle",
      icon: "x",
      label: _t("general.showSettingsCloseButton"),
      desc: _t("general.showSettingsCloseButtonDescription"),
      get: () => s.showSettingsCloseButton,
      set: (v) => generalSettings.updateSetting("showSettingsCloseButton", v),
    },
  ]);

  const windowEntries: SettingEntryConfig[] = $derived([
    {
      type: "slider",
      icon: "sliders",
      label: _t("general.windowTransparency"),
      get: () => s.windowTransparency,
      set: (v) => generalSettings.updateSetting("windowTransparency", v),
      min: 60,
      max: 100,
      suffix: "%",
    },
    {
      type: "select",
      icon: "sliders",
      label: _t("general.windowEffect"),
      desc: _t("general.windowEffectDescription"),
      get: () => s.windowEffect,
      options: [
        { value: "off", label: _t("general.windowEffectOff") },
        { value: "acrylic", label: _t("general.windowEffectAcrylic") },
        { value: "mica", label: _t("general.windowEffectMica") },
      ],
      set: (v) => generalSettings.updateSetting("windowEffect", v as WindowEffect),
    },
    {
      type: "toggle",
      icon: "eye",
      label: _t("general.windowOpacityAffectsText"),
      desc: _t("general.windowOpacityAffectsTextDescription"),
      get: () => s.windowOpacityAffectsText,
      set: (v) => generalSettings.updateSetting("windowOpacityAffectsText", v),
    },
    {
      type: "toggle",
      icon: "pin",
      label: _t("general.rememberWindowPosition"),
      desc: _t("general.rememberWindowPositionDescription"),
      get: () => s.rememberWindowPosition,
      set: (v) => generalSettings.updateSetting("rememberWindowPosition", v),
    },
    {
      type: "select",
      icon: "grid",
      label: _t("general.detailDisplayMode"),
      desc: _t("general.detailDisplayModeDescription"),
      get: () => s.detailDisplayMode,
      options: [
        { value: "overlay", label: _t("general.detailDisplayModeOverlay") },
        { value: "split", label: _t("general.detailDisplayModeSplit") },
      ],
      set: (v) => generalSettings.updateSetting("detailDisplayMode", v as "overlay" | "split"),
    },
    {
      type: "toggle",
      icon: "maximize",
      label: _t("general.desktopFullscreen"),
      desc: _t("general.desktopFullscreenDescription"),
      get: () => s.imageFullscreenMode === "desktop",
      set: (v) => generalSettings.updateSetting("imageFullscreenMode", v ? "desktop" : "overlay"),
    },
    {
      type: "slider",
      icon: "image",
      label: _t("general.viewerBackdropOpacity"),
      get: () => s.viewerBackdropOpacity,
      set: (v) => generalSettings.updateSetting("viewerBackdropOpacity", v),
      min: 0,
      max: 100,
      suffix: "%",
    },
  ]);
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("general.eyebrow")}</span>
      <h2>{_t("general.title")}</h2>
      <p>{_t("general.description")}</p>
    </div>
    {#if s.showSettingsCloseButton}
      <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
        >×</button
      >
    {/if}
  </header>
{/if}

<div class="settings-scroll">
  {#if section === "search"}
    {#each searchEntries as config}
      <SettingEntry {config}
        >{#snippet children()}
          {#if config.type === "custom" && config.id === "general.search-sort-rules"}
            <div class="sort-rules-list" role="list" bind:this={sortListEl}>
              {#each s.searchSortRules as rule, idx (idx)}
                <div
                  class="sort-rule-row"
                  class:sort-dragging={sortDragIdx === idx}
                  role="listitem"
                >
                  <span
                    class="sort-grip"
                    role="button"
                    tabindex="0"
                    aria-label={_t("general.sortDragHandle")}
                    onpointerdown={(e) => pointerDragStart(idx, e)}
                  >
                    <span class="grip-dot"></span>
                    <span class="grip-dot"></span>
                    <span class="grip-dot"></span>
                    <span class="grip-dot"></span>
                  </span>
                  <CustomSelect
                    className="sort-field-select"
                    value={rule.field}
                    ariaLabel={_t("general.searchSortRules")}
                    options={ALL_SORT_FIELDS.map((f) => ({
                      value: f,
                      label: _t(SORT_FIELD_LABELS[f]),
                      disabled: s.searchSortRules.some(
                        (r: SortRule, i: number) => i !== idx && r.field === f,
                      ),
                    }))}
                    onchange={(v) => {
                      const newRules = [...s.searchSortRules];
                      newRules[idx] = { ...rule, field: v as SortRule["field"] };
                      generalSettings.updateSetting("searchSortRules", newRules);
                    }}
                  />
                  <button
                    type="button"
                    class="sort-direction-btn"
                    title={rule.direction === "asc"
                      ? _t("general.sortAsc")
                      : _t("general.sortDesc")}
                    aria-label={rule.direction === "asc"
                      ? _t("general.sortAsc")
                      : _t("general.sortDesc")}
                    onclick={() => {
                      const newRules = [...s.searchSortRules];
                      newRules[idx] = {
                        ...rule,
                        direction: rule.direction === "asc" ? "desc" : ("asc" as const),
                      };
                      generalSettings.updateSetting("searchSortRules", newRules);
                    }}
                  >
                    {rule.direction === "asc" ? "↑" : "↓"}
                  </button>
                  {#if s.searchSortRules.length > 1}
                    <button
                      type="button"
                      class="sort-remove-btn"
                      title={_t("general.sortRemoveRule")}
                      aria-label={_t("general.sortRemoveRule")}
                      onclick={() => {
                        const newRules = s.searchSortRules.filter((_, i) => i !== idx);
                        generalSettings.updateSetting("searchSortRules", newRules);
                      }}>×</button
                    >
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        {/snippet}
      </SettingEntry>
    {/each}
  {:else if section === "items"}
    {#each itemsEntries as config}
      <SettingEntry {config} />
    {/each}
  {:else if section === "general"}
    {#each generalEntries as config}
      <SettingEntry {config}>
        {#snippet children()}
          {#if config.type === "custom" && config.id === "general.language"}
            <div class="lang-toggle">
              <button
                type="button"
                class:active={s.language === "zh-CN"}
                onclick={() => changeLanguage("zh-CN")}>中文</button
              >
              <button
                type="button"
                class:active={s.language === "en"}
                onclick={() => changeLanguage("en")}>English</button
              >
            </div>
          {:else if config.type === "custom" && config.id === "recording.pause"}
            <div class="pause-control">
              <span class="pause-state"
                >{_t(privacyPaused ? "capture.paused" : "capture.active")}</span
              >
              <button
                type="button"
                class="toggle-switch"
                class:active={!privacyPaused}
                role="switch"
                aria-checked={!privacyPaused}
                aria-label={_t(privacyPaused ? "capture.resumeAction" : "capture.pauseAction")}
                title={_t(privacyPaused ? "capture.resumeAction" : "capture.pauseAction")}
                disabled={privacyLoading || !isTauriRuntime()}
                onclick={togglePrivacyPause}
              >
                <span class="toggle-knob"></span>
              </button>
            </div>
          {/if}
        {/snippet}
      </SettingEntry>
    {/each}
  {:else}
    {#each windowEntries as config}
      <SettingEntry {config} />
    {/each}
  {/if}

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>

{#if feedback}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  .lang-toggle {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .lang-toggle button {
    padding: 7px 16px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-muted);
    background: var(--input-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .lang-toggle button:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .lang-toggle button.active {
    border-color: var(--selection-color);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }

  .pause-control {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 0 0 auto;
  }

  .pause-state {
    color: var(--text-muted);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  .sort-rules-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 8px 0 0;
  }

  .sort-rule-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
    border-radius: var(--settings-control-radius, 6px);
  }

  .sort-rule-row.sort-dragging {
    opacity: 0.4;
  }

  :global(.sort-drag-over) {
    outline: 2px solid var(--accent) !important;
    outline-offset: 2px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }

  .sort-grip {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    padding: 4px;
    cursor: grab;
    flex-shrink: 0;
    border-radius: 4px;
    align-self: stretch;
    align-content: center;
    user-select: none;
    touch-action: none;
  }

  .sort-grip:hover {
    background: var(--hover-bg);
  }

  .sort-grip:active {
    cursor: grabbing;
  }

  .grip-dot {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--text-muted);
    display: block;
  }

  :global(.sort-field-select) {
    flex: 1;
    min-width: 0;
  }

  .sort-direction-btn {
    width: 32px;
    height: 28px;
    border-radius: var(--settings-control-radius, 6px);
    border: 1px solid var(--border-color);
    background: var(--input-bg);
    color: var(--text-primary);
    font-size: 14px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .sort-direction-btn:hover {
    background: var(--hover-bg);
  }

  .sort-remove-btn {
    width: 28px;
    height: 28px;
    border-radius: var(--settings-control-radius, 6px);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    flex-shrink: 0;
  }

  .sort-remove-btn:hover {
    color: var(--danger-color);
    border-color: var(--danger-color);
  }
</style>
